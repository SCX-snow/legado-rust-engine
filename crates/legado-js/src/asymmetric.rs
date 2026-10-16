use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::function::{Opt, Rest};
use rquickjs::{Ctx, Function, Object, Value as JsValue};
use rsa::pkcs1::{DecodeRsaPrivateKey as _, DecodeRsaPublicKey as _};
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey as _, DecodePublicKey as _};
use rsa::signature::{SignatureEncoding as _, Signer as _, Verifier as _};
use rsa::{Oaep, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};

use crate::crypto::decode_any;
use crate::java::{bytes_to_js, js_bytes, js_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeySide {
    Public,
    Private,
}

pub(crate) fn key_side(value: Option<&JsValue<'_>>, default: KeySide) -> KeySide {
    match value {
        Some(value) if value.is_bool() => match value.as_bool() {
            Some(true) => KeySide::Public,
            _ => KeySide::Private,
        },
        Some(value) if !value.is_null() && !value.is_undefined() => {
            let text = js_text(value).to_ascii_lowercase();
            if text.contains("public") {
                KeySide::Public
            } else if text.contains("private") {
                KeySide::Private
            } else {
                default
            }
        }
        _ => default,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RsaPadding {
    Pkcs1,
    OaepSha256,
}

impl RsaPadding {
    pub(crate) fn parse(transformation: &str) -> Result<Self, String> {
        let mut parts = transformation.split('/');
        let algorithm = parts.next().unwrap_or_default().trim().to_ascii_uppercase();
        if !algorithm.starts_with("RSA") {
            return Err(format!("还不支持的非对称算法 {algorithm}（只有 RSA）"));
        }
        let rest = transformation.to_ascii_uppercase();
        if rest.contains("OAEP") {
            Ok(Self::OaepSha256)
        } else {
            Ok(Self::Pkcs1)
        }
    }
}

#[derive(Default)]
pub(crate) struct RsaKeys {
    private: Option<RsaPrivateKey>,
    public: Option<RsaPublicKey>,
}

impl RsaKeys {
    pub(crate) fn set_private(&mut self, key: RsaPrivateKey) {
        self.public = Some(RsaPublicKey::from(&key));
        self.private = Some(key);
    }

    pub(crate) fn set_public(&mut self, key: RsaPublicKey) {
        self.public = Some(key);
    }

    fn public(&self) -> Result<&RsaPublicKey, String> {
        self.public.as_ref().ok_or_else(|| "还没有公钥".to_string())
    }

    fn private(&self) -> Result<&RsaPrivateKey, String> {
        self.private
            .as_ref()
            .ok_or_else(|| "还没有私钥".to_string())
    }
}

pub(crate) fn asymmetric_object<'js>(
    ctx: &Ctx<'js>,
    keys: Rc<RefCell<RsaKeys>>,
    padding: RsaPadding,
) -> Object<'js> {
    let object = Object::new(ctx.clone()).expect("建非对称加密对象");

    for (name, public) in [("setPublicKey", true), ("setPrivateKey", false)] {
        object
            .set(
                name,
                Function::new(ctx.clone(), {
                    let keys = keys.clone();
                    let object = object.clone();
                    move |value: JsValue<'js>| -> Object<'js> {
                        let der = key_der(&value);
                        if public {
                            if let Ok(key) = parse_public(&der) {
                                keys.borrow_mut().set_public(key);
                            }
                        } else if let Ok(key) = parse_private(&der) {
                            keys.borrow_mut().set_private(key);
                        }
                        object.clone()
                    }
                })
                .expect("建 setKey"),
            )
            .expect("挂 setKey");
    }

    for (name, base64) in [("encryptBase64", true), ("encryptHex", false)] {
        object
            .set(
                name,
                Function::new(ctx.clone(), {
                    let keys = keys.clone();
                    move |data: JsValue<'js>, side: Opt<JsValue<'js>>| -> rquickjs::Result<String> {
                        if key_side(side.0.as_ref(), KeySide::Public) != KeySide::Public {
                            return Err(js_error("加密要用公钥".to_string()));
                        }
                        let bytes = encrypt(&keys, padding, &js_bytes(&data)).map_err(js_error)?;
                        use base64::Engine as _;
                        Ok(if base64 {
                            base64::engine::general_purpose::STANDARD.encode(bytes)
                        } else {
                            hex::encode(bytes)
                        })
                    }
                })
                .expect("建 encryptStr"),
            )
            .expect("挂 encryptStr");
    }

    object
        .set(
            "encrypt",
            Function::new(ctx.clone(), {
                let keys = keys.clone();
                move |ctx: Ctx<'js>, data: JsValue<'js>, side: Opt<JsValue<'js>>| {
                    if key_side(side.0.as_ref(), KeySide::Public) != KeySide::Public {
                        return Err(js_error("加密要用公钥".to_string()));
                    }
                    let bytes = encrypt(&keys, padding, &js_bytes(&data)).map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &bytes))
                }
            })
            .expect("建 encrypt"),
        )
        .expect("挂 encrypt");
    object
        .set(
            "decrypt",
            Function::new(ctx.clone(), {
                let keys = keys.clone();
                move |ctx: Ctx<'js>, data: JsValue<'js>, side: Opt<JsValue<'js>>| {
                    let bytes = decrypt_input(&data).map_err(js_error)?;
                    if key_side(side.0.as_ref(), KeySide::Private) != KeySide::Private {
                        return Err(js_error("解密要用私钥".to_string()));
                    }
                    let out = decrypt(&keys, padding, &bytes).map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &out))
                }
            })
            .expect("建 decrypt"),
        )
        .expect("挂 decrypt");
    object
        .set(
            "decryptStr",
            Function::new(ctx.clone(), {
                let keys = keys.clone();
                move |data: JsValue<'js>, side: Opt<JsValue<'js>>| -> rquickjs::Result<String> {
                    let bytes = decrypt_input(&data).map_err(js_error)?;
                    if key_side(side.0.as_ref(), KeySide::Private) != KeySide::Private {
                        return Err(js_error("解密要用私钥".to_string()));
                    }
                    let out = decrypt(&keys, padding, &bytes).map_err(js_error)?;
                    Ok(String::from_utf8_lossy(&out).into_owned())
                }
            })
            .expect("建 decryptStr"),
        )
        .expect("挂 decryptStr");
    object
}

pub(crate) fn rsa_class<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<JsValue<'js>>| -> rquickjs::Result<Object<'js>> {
            let keys = Rc::new(RefCell::new(RsaKeys::default()));
            if let Some(private) = args.0.first() {
                let der = key_der(private);
                let key = parse_private(&der).map_err(js_error)?;
                keys.borrow_mut().set_private(key);
            }
            if let Some(public) = args.0.get(1) {
                let der = key_der(public);
                let key = parse_public(&der).map_err(js_error)?;
                keys.borrow_mut().set_public(key);
            }
            Ok(asymmetric_object(&ctx, keys, RsaPadding::Pkcs1))
        },
    )
}

pub(crate) fn key_type<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("PublicKey", "PublicKey")?;
    object.set("PrivateKey", "PrivateKey")?;
    Ok(object)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SignAlgorithm {
    Sha256,
    Sha1,
    Sha512,
    Md5,
}

impl SignAlgorithm {
    fn parse(algorithm: &str) -> Result<Self, String> {
        let text = algorithm
            .trim()
            .to_ascii_uppercase()
            .replace(['-', '_'], "");
        if text.contains("MD5") {
            Ok(Self::Md5)
        } else if text.contains("SHA1") || text.contains("SHA_1") {
            Ok(Self::Sha1)
        } else if text.contains("SHA512") {
            Ok(Self::Sha512)
        } else if text.contains("SHA256") {
            Ok(Self::Sha256)
        } else {
            Err(format!(
                "还不支持的签名算法 {algorithm}（只有 MD5/SHA1/SHA256/SHA512 with RSA）"
            ))
        }
    }
}

fn sign_object<'js>(ctx: &Ctx<'js>, algorithm: SignAlgorithm) -> rquickjs::Result<Object<'js>> {
    let keys = Rc::new(RefCell::new(RsaKeys::default()));
    let object = Object::new(ctx.clone())?;
    for (name, public) in [("setPublicKey", true), ("setPrivateKey", false)] {
        object.set(
            name,
            Function::new(ctx.clone(), {
                let keys = keys.clone();
                let object = object.clone();
                move |value: JsValue<'js>| -> Object<'js> {
                    let der = key_der(&value);
                    if public {
                        if let Ok(key) = parse_public(&der) {
                            keys.borrow_mut().set_public(key);
                        }
                    } else if let Ok(key) = parse_private(&der) {
                        keys.borrow_mut().set_private(key);
                    }
                    object.clone()
                }
            })?,
        )?;
    }
    object.set(
        "sign",
        Function::new(ctx.clone(), {
            let keys = keys.clone();
            move |ctx: Ctx<'js>, data: JsValue<'js>, side: Opt<JsValue<'js>>| {
                if key_side(side.0.as_ref(), KeySide::Private) != KeySide::Private {
                    return Err(js_error("签名要用私钥".to_string()));
                }
                let bytes = sign(&keys, algorithm, &js_bytes(&data)).map_err(js_error)?;
                Ok(bytes_to_js(&ctx, &bytes))
            }
        })?,
    )?;
    object.set(
        "signHex",
        Function::new(ctx.clone(), {
            let keys = keys.clone();
            move |data: JsValue<'js>, side: Opt<JsValue<'js>>| -> rquickjs::Result<String> {
                if key_side(side.0.as_ref(), KeySide::Private) != KeySide::Private {
                    return Err(js_error("签名要用私钥".to_string()));
                }
                Ok(hex::encode(
                    sign(&keys, algorithm, &js_bytes(&data)).map_err(js_error)?,
                ))
            }
        })?,
    )?;
    object.set(
        "verify",
        Function::new(ctx.clone(), {
            let keys = keys.clone();
            move |data: JsValue<'js>, signature: JsValue<'js>| -> rquickjs::Result<bool> {
                let signature = decode_any(&js_text(&signature))
                    .map_err(|error| js_error(format!("签名不是 hex / base64: {error}")))?;
                verify(&keys, algorithm, &js_bytes(&data), &signature).map_err(js_error)
            }
        })?,
    )?;
    Ok(object)
}

fn sign(
    keys: &Rc<RefCell<RsaKeys>>,
    algorithm: SignAlgorithm,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    let key = keys.borrow().private()?.clone();
    let signature = match algorithm {
        SignAlgorithm::Sha256 => sign_with::<sha2::Sha256>(key, data),
        SignAlgorithm::Sha1 => sign_with::<sha1::Sha1>(key, data),
        SignAlgorithm::Sha512 => sign_with::<sha2::Sha512>(key, data),
        SignAlgorithm::Md5 => sign_with::<md5::Md5>(key, data),
    }?;
    Ok(signature.to_vec())
}

fn sign_with<D>(key: RsaPrivateKey, data: &[u8]) -> Result<Signature, String>
where
    D: sha2::Digest + rsa::pkcs8::AssociatedOid,
{
    SigningKey::<D>::new(key)
        .try_sign(data)
        .map_err(|error| format!("签名失败: {error}"))
}

fn verify(
    keys: &Rc<RefCell<RsaKeys>>,
    algorithm: SignAlgorithm,
    data: &[u8],
    signature: &[u8],
) -> Result<bool, String> {
    let key = keys.borrow().public()?.clone();
    let signature =
        Signature::try_from(signature).map_err(|error| format!("签名长度不对: {error}"))?;
    match algorithm {
        SignAlgorithm::Sha256 => verify_with::<sha2::Sha256>(key, data, signature),
        SignAlgorithm::Sha1 => verify_with::<sha1::Sha1>(key, data, signature),
        SignAlgorithm::Sha512 => verify_with::<sha2::Sha512>(key, data, signature),
        SignAlgorithm::Md5 => verify_with::<md5::Md5>(key, data, signature),
    }
}

fn verify_with<D>(key: RsaPublicKey, data: &[u8], signature: Signature) -> Result<bool, String>
where
    D: sha2::Digest + rsa::pkcs8::AssociatedOid,
{
    Ok(VerifyingKey::<D>::new(key).verify(data, &signature).is_ok())
}

pub(crate) fn create_sign<'js>(ctx: &Ctx<'js>, algorithm: &str) -> rquickjs::Result<Object<'js>> {
    sign_object(ctx, SignAlgorithm::parse(algorithm).map_err(js_error)?)
}

pub(crate) fn sign_class<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>, algorithm: String| {
        create_sign(&ctx, &algorithm)
    })
}

fn encrypt(
    keys: &Rc<RefCell<RsaKeys>>,
    padding: RsaPadding,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    let key = keys.borrow().public()?.clone();
    let mut rng = rand::thread_rng();
    match padding {
        RsaPadding::Pkcs1 => key.encrypt(&mut rng, Pkcs1v15Encrypt, data),
        RsaPadding::OaepSha256 => key.encrypt(&mut rng, Oaep::new::<sha2::Sha256>(), data),
    }
    .map_err(|error| format!("RSA 加密失败: {error}"))
}

fn decrypt(
    keys: &Rc<RefCell<RsaKeys>>,
    padding: RsaPadding,
    data: &[u8],
) -> Result<Vec<u8>, String> {
    let key = keys.borrow().private()?.clone();
    match padding {
        RsaPadding::Pkcs1 => key.decrypt(Pkcs1v15Encrypt, data),
        RsaPadding::OaepSha256 => key.decrypt(Oaep::new::<sha2::Sha256>(), data),
    }
    .map_err(|error| format!("RSA 解密失败（密钥或填充方式不对）: {error}"))
}

fn decrypt_input(value: &JsValue<'_>) -> Result<Vec<u8>, String> {
    if value.as_array().is_some() {
        return Ok(js_bytes(value));
    }
    decode_any(&js_text(value))
}

fn key_der(value: &JsValue<'_>) -> Vec<u8> {
    let Some(text) = value.as_string().and_then(|text| text.to_string().ok()) else {
        return js_bytes(value);
    };
    if text.contains("-----BEGIN") {
        let body: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("-----"))
            .collect::<Vec<_>>()
            .join("");
        return decode_any(&body).unwrap_or_default();
    }
    decode_any(&text).unwrap_or_else(|_| text.into_bytes())
}

fn parse_private(der: &[u8]) -> Result<RsaPrivateKey, String> {
    RsaPrivateKey::from_pkcs8_der(der)
        .or_else(|_| RsaPrivateKey::from_pkcs1_der(der))
        .map_err(|error| format!("私钥解析失败（要 PKCS#8 或 PKCS#1）: {error}"))
}

fn parse_public(der: &[u8]) -> Result<RsaPublicKey, String> {
    RsaPublicKey::from_public_key_der(der)
        .or_else(|_| RsaPublicKey::from_pkcs1_der(der))
        .map_err(|error| format!("公钥解析失败（要 X.509 或 PKCS#1）: {error}"))
}

fn js_error(message: String) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("宿主", "asymmetric", message)
}
