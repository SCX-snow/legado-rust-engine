use std::rc::Rc;

use aes::cipher::block_padding::NoPadding as NoPad;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};
use rquickjs::function::{Opt, Rest};
use rquickjs::{Ctx, Function, Object, Value as JsValue};

use crate::java::{bytes_to_js, decode_base64, js_bytes, js_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Algorithm {
    Aes,
    Des,
    DesEde,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Cbc,
    Ecb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Padding {
    Pkcs5,
    None,
    Zero,
}

macro_rules! cipher_ops {
    ($name:ident, $cipher:ty) => {
        mod $name {
            use super::*;

            pub(super) fn cbc_encrypt(
                key: &[u8],
                iv: &[u8],
                data: &[u8],
            ) -> Result<Vec<u8>, String> {
                Ok(cbc::Encryptor::<$cipher>::new_from_slices(key, iv)
                    .map_err(|_| "密钥或 IV 长度不对".to_string())?
                    .encrypt_padded_vec_mut::<NoPad>(data))
            }

            pub(super) fn cbc_decrypt(
                key: &[u8],
                iv: &[u8],
                data: &[u8],
            ) -> Result<Vec<u8>, String> {
                cbc::Decryptor::<$cipher>::new_from_slices(key, iv)
                    .map_err(|_| "密钥或 IV 长度不对".to_string())?
                    .decrypt_padded_vec_mut::<NoPad>(data)
                    .map_err(|_| "密文长度不是分组的整数倍".to_string())
            }

            pub(super) fn ecb_encrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
                Ok(ecb::Encryptor::<$cipher>::new_from_slice(key)
                    .map_err(|_| "密钥长度不对".to_string())?
                    .encrypt_padded_vec_mut::<NoPad>(data))
            }

            pub(super) fn ecb_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
                ecb::Decryptor::<$cipher>::new_from_slice(key)
                    .map_err(|_| "密钥长度不对".to_string())?
                    .decrypt_padded_vec_mut::<NoPad>(data)
                    .map_err(|_| "密文长度不是分组的整数倍".to_string())
            }
        }
    };
}

cipher_ops!(aes128, aes::Aes128);
cipher_ops!(aes192, aes::Aes192);
cipher_ops!(aes256, aes::Aes256);
cipher_ops!(des_ops, des::Des);
cipher_ops!(des_ede3_ops, des::TdesEde3);

macro_rules! dispatch {
    ($module:ident, $key:expr, $iv:expr, $data:expr, $mode:expr, $encrypt:expr) => {
        match ($mode, $encrypt) {
            (Mode::Cbc, true) => $module::cbc_encrypt($key, $iv, $data),
            (Mode::Cbc, false) => $module::cbc_decrypt($key, $iv, $data),
            (Mode::Ecb, true) => $module::ecb_encrypt($key, $data),
            (Mode::Ecb, false) => $module::ecb_decrypt($key, $data),
        }
    };
}

pub(crate) struct Symmetric {
    algorithm: Algorithm,
    mode: Mode,
    padding: Padding,
    key: Vec<u8>,
    iv: Vec<u8>,
}

impl Symmetric {
    pub(crate) fn new(
        algorithm: Algorithm,
        mode: Mode,
        padding: Padding,
        key: Vec<u8>,
        iv: Vec<u8>,
    ) -> Result<Self, String> {
        if mode == Mode::Cbc && iv.is_empty() {
            return Err("CBC 模式要显式给 IV".to_string());
        }
        Ok(Self {
            algorithm,
            mode,
            padding,
            key,
            iv,
        })
    }

    pub(crate) fn block_size(&self) -> usize {
        match self.algorithm {
            Algorithm::Aes => 16,
            Algorithm::Des | Algorithm::DesEde => 8,
        }
    }

    fn core(&self, encrypt: bool, data: &[u8]) -> Result<Vec<u8>, String> {
        let (key, iv, mode) = (self.key.as_slice(), self.iv.as_slice(), self.mode);
        match self.algorithm {
            Algorithm::Aes => match key.len() {
                16 => dispatch!(aes128, key, iv, data, mode, encrypt),
                24 => dispatch!(aes192, key, iv, data, mode, encrypt),
                32 => dispatch!(aes256, key, iv, data, mode, encrypt),
                other => Err(format!(
                    "AES 密钥长度 {other} 不支持（要 16 / 24 / 32 字节）"
                )),
            },
            Algorithm::Des => match key.len() {
                8 => dispatch!(des_ops, key, iv, data, mode, encrypt),
                other => Err(format!("DES 密钥长度 {other} 不支持（要 8 字节）")),
            },
            Algorithm::DesEde => match key.len() {
                24 => dispatch!(des_ede3_ops, key, iv, data, mode, encrypt),
                other => Err(format!("DESede 密钥长度 {other} 不支持（要 24 字节）")),
            },
        }
    }

    pub(crate) fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let block = self.block_size();
        let padded = self.pad(data, block);
        self.core(true, &padded)
    }

    pub(crate) fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let out = self.core(false, data)?;
        self.unpad(out)
    }

    fn pad(&self, data: &[u8], block: usize) -> Vec<u8> {
        let mut out = data.to_vec();
        let pad = block - data.len() % block;
        match self.padding {
            Padding::None => {}
            Padding::Pkcs5 => out.extend(std::iter::repeat_n(pad as u8, pad)),
            Padding::Zero => out.extend(std::iter::repeat_n(0u8, pad)),
        }
        out
    }

    fn unpad(&self, data: Vec<u8>) -> Result<Vec<u8>, String> {
        let block = self.block_size();
        match self.padding {
            Padding::None => Ok(data),
            Padding::Pkcs5 => {
                let Some(&last) = data.last() else {
                    return Ok(data);
                };
                let count = last as usize;
                if count == 0 || count > block || count > data.len() {
                    return Err("PKCS5 填充不合法（密钥或 IV 可能不对）".to_string());
                }
                if data[data.len() - count..].iter().any(|byte| *byte != last) {
                    return Err("PKCS5 填充不合法（密钥或 IV 可能不对）".to_string());
                }
                Ok(data[..data.len() - count].to_vec())
            }
            Padding::Zero => Ok(data
                .iter()
                .rposition(|byte| *byte != 0)
                .map(|at| data[..=at].to_vec())
                .unwrap_or_default()),
        }
    }
}

pub(crate) fn decode_any(text: &str) -> Result<Vec<u8>, String> {
    let trimmed = text.trim();
    let is_hex = trimmed.len().is_multiple_of(2)
        && !trimmed.is_empty()
        && trimmed.chars().all(|ch| ch.is_ascii_hexdigit());
    if is_hex {
        return hex::decode(trimmed).map_err(|error| format!("hex 解码失败: {error}"));
    }
    decode_base64(trimmed).ok_or_else(|| "既不是合法的 hex 也不是 base64".to_string())
}

pub(crate) fn class<'js>(ctx: &Ctx<'js>, algorithm: Algorithm) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<JsValue<'js>>| -> rquickjs::Result<Object<'js>> {
            let symmetric = parse_args(algorithm, &args.0).map_err(js_error)?;
            Ok(crypto_object(&ctx, Rc::new(symmetric)))
        },
    )
}

fn js_error(message: String) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("宿主", "crypto", message)
}

fn parse_args(algorithm: Algorithm, args: &[JsValue<'_>]) -> Result<Symmetric, String> {
    let value_bytes = |value: &JsValue<'_>| -> Vec<u8> {
        match value.as_string() {
            Some(text) => text.to_string().unwrap_or_default().into_bytes(),
            None => js_bytes(value),
        }
    };
    let (mode, padding, key, iv) = match args.len() {
        1 => (Mode::Ecb, Padding::Pkcs5, value_bytes(&args[0]), Vec::new()),
        2 => (
            parse_mode(&js_text(&args[0]))?,
            Padding::Pkcs5,
            value_bytes(&args[1]),
            Vec::new(),
        ),
        3 => (
            parse_mode(&js_text(&args[0]))?,
            parse_padding(&js_text(&args[1]))?,
            value_bytes(&args[2]),
            Vec::new(),
        ),
        _ => (
            parse_mode(&js_text(&args[0]))?,
            parse_padding(&js_text(&args[1]))?,
            value_bytes(&args[2]),
            value_bytes(&args[3]),
        ),
    };
    Symmetric::new(algorithm, mode, padding, key, iv)
}

fn parse_mode(text: &str) -> Result<Mode, String> {
    match text.trim().to_ascii_uppercase().as_str() {
        "CBC" => Ok(Mode::Cbc),
        "ECB" => Ok(Mode::Ecb),
        other => Err(format!("还不支持的模式 {other}（只有 CBC / ECB）")),
    }
}

fn parse_padding(text: &str) -> Result<Padding, String> {
    match text.trim().to_ascii_uppercase().replace('_', "").as_str() {
        "PKCS5PADDING" | "PKCS7PADDING" => Ok(Padding::Pkcs5),
        "NOPADDING" => Ok(Padding::None),
        "ZEROPADDING" => Ok(Padding::Zero),
        other => Err(format!(
            "还不支持的填充 {other}（只有 PKCS5/7、NoPadding、ZeroPadding）"
        )),
    }
}

pub(crate) fn crypto_object<'js>(ctx: &Ctx<'js>, symmetric: Rc<Symmetric>) -> Object<'js> {
    let object = Object::new(ctx.clone()).expect("建加密对象");
    object
        .set(
            "decryptStr",
            Function::new(ctx.clone(), {
                let symmetric = symmetric.clone();
                move |data: String, charset: Opt<String>| -> rquickjs::Result<String> {
                    let bytes = symmetric
                        .decrypt(&decode_any(&data).map_err(js_error)?)
                        .map_err(js_error)?;
                    Ok(decode_charset(&bytes, charset.0.as_deref()))
                }
            })
            .expect("建 decryptStr"),
        )
        .expect("挂 decryptStr");
    object
        .set(
            "decrypt",
            Function::new(ctx.clone(), {
                let symmetric = symmetric.clone();
                move |ctx: Ctx<'js>, data: String| -> rquickjs::Result<JsValue<'js>> {
                    let bytes = symmetric
                        .decrypt(&decode_any(&data).map_err(js_error)?)
                        .map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &bytes))
                }
            })
            .expect("建 decrypt"),
        )
        .expect("挂 decrypt");
    object
        .set(
            "encryptStr",
            Function::new(ctx.clone(), {
                let symmetric = symmetric.clone();
                move |data: JsValue<'_>| -> rquickjs::Result<String> {
                    let bytes = symmetric.encrypt(&js_bytes(&data)).map_err(js_error)?;
                    use base64::Engine as _;
                    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
                }
            })
            .expect("建 encryptStr"),
        )
        .expect("挂 encryptStr");
    object
        .set(
            "encryptHex",
            Function::new(ctx.clone(), {
                let symmetric = symmetric.clone();
                move |data: JsValue<'_>| -> rquickjs::Result<String> {
                    Ok(hex::encode(
                        symmetric.encrypt(&js_bytes(&data)).map_err(js_error)?,
                    ))
                }
            })
            .expect("建 encryptHex"),
        )
        .expect("挂 encryptHex");
    object
        .set(
            "encrypt",
            Function::new(ctx.clone(), {
                let symmetric = symmetric.clone();
                move |ctx: Ctx<'js>, data: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                    let bytes = symmetric.encrypt(&js_bytes(&data)).map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &bytes))
                }
            })
            .expect("建 encrypt"),
        )
        .expect("挂 encrypt");
    object
}

pub(crate) fn decode_charset(bytes: &[u8], charset: Option<&str>) -> String {
    let Some(label) = charset else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    match encoding_rs::Encoding::for_label(label.trim().as_bytes()) {
        Some(encoding) => encoding.decode(bytes).0.into_owned(),
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

pub(crate) fn parse_transformation(text: &str) -> Result<(Algorithm, Mode, Padding), String> {
    let parts: Vec<&str> = text.split('/').collect();
    let algorithm = parse_algorithm(parts.first().copied().unwrap_or_default())?;
    let mode = match parts.get(1) {
        Some(mode) => parse_mode(mode)?,
        None => Mode::Ecb,
    };
    let padding = match parts.get(2) {
        Some(padding) => parse_padding(padding)?,
        None => Padding::Pkcs5,
    };
    Ok((algorithm, mode, padding))
}

fn parse_algorithm(text: &str) -> Result<Algorithm, String> {
    match text
        .trim()
        .to_ascii_uppercase()
        .replace(['-', '_'], "")
        .as_str()
    {
        "AES" => Ok(Algorithm::Aes),
        "DES" => Ok(Algorithm::Des),
        "DESEDE" | "TRIPLEDES" | "3DES" => Ok(Algorithm::DesEde),
        other => Err(format!("还不支持的算法 {other}（只有 AES / DES / DESede）")),
    }
}

pub(crate) fn digest_bytes(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use sha1::Sha1;
    use sha2::{Digest as _, Sha256, Sha512};
    match normalize(algorithm).as_str() {
        "MD5" => {
            use md5::{Digest as _, Md5};
            let mut hasher = Md5::new();
            hasher.update(data);
            Ok(hasher.finalize().to_vec())
        }
        "SHA1" => {
            let mut hasher = Sha1::new();
            hasher.update(data);
            Ok(hasher.finalize().to_vec())
        }
        "SHA256" => {
            let mut hasher = Sha256::new();
            hasher.update(data);
            Ok(hasher.finalize().to_vec())
        }
        "SHA512" => {
            let mut hasher = Sha512::new();
            hasher.update(data);
            Ok(hasher.finalize().to_vec())
        }
        other => Err(format!(
            "还不支持的摘要算法 {other}（只有 MD5 / SHA1 / SHA256 / SHA512）"
        )),
    }
}

pub(crate) fn check_hmac_algorithm(algorithm: &str) -> Result<(), String> {
    match normalize(algorithm).as_str() {
        "MD5" | "SHA1" | "SHA256" | "SHA512" => Ok(()),
        other => Err(format!(
            "还不支持的 HMAC 算法 {other}（只有 HmacMD5 / SHA1 / SHA256 / SHA512）"
        )),
    }
}

pub(crate) fn hmac_bytes(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use hmac::{Hmac, Mac as _};
    match normalize(algorithm).as_str() {
        "MD5" => {
            let mut mac = <Hmac<md5::Md5> as hmac::Mac>::new_from_slice(key)
                .map_err(|_| "HMAC 密钥长度不对".to_string())?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "SHA1" => {
            let mut mac = <Hmac<sha1::Sha1> as hmac::Mac>::new_from_slice(key)
                .map_err(|_| "HMAC 密钥长度不对".to_string())?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "SHA256" => {
            let mut mac = <Hmac<sha2::Sha256> as hmac::Mac>::new_from_slice(key)
                .map_err(|_| "HMAC 密钥长度不对".to_string())?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "SHA512" => {
            let mut mac = <Hmac<sha2::Sha512> as hmac::Mac>::new_from_slice(key)
                .map_err(|_| "HMAC 密钥长度不对".to_string())?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        other => Err(format!(
            "还不支持的 HMAC 算法 {other}（只有 HmacMD5 / SHA1 / SHA256 / SHA512）"
        )),
    }
}

fn normalize(algorithm: &str) -> String {
    let upper = algorithm.trim().to_ascii_uppercase();
    let stripped = upper.strip_prefix("HMAC").unwrap_or(&upper);
    stripped.replace(['-', '_'], "")
}

pub(crate) fn digest_util<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    for (name, algorithm) in [
        ("md5Hex", "md5"),
        ("sha1Hex", "sha1"),
        ("sha256Hex", "sha256"),
        ("sha512Hex", "sha512"),
    ] {
        object.set(
            name,
            Function::new(
                ctx.clone(),
                move |data: String| -> rquickjs::Result<String> {
                    Ok(hex::encode(
                        digest_bytes(algorithm, data.as_bytes()).map_err(js_error)?,
                    ))
                },
            )?,
        )?;
    }
    object.set(
        "digestHex",
        Function::new(
            ctx.clone(),
            |data: String, algorithm: String| -> rquickjs::Result<String> {
                Ok(hex::encode(
                    digest_bytes(&algorithm, data.as_bytes()).map_err(js_error)?,
                ))
            },
        )?,
    )?;
    object.set(
        "digestBase64",
        Function::new(
            ctx.clone(),
            |data: String, algorithm: String| -> rquickjs::Result<String> {
                use base64::Engine as _;
                Ok(base64::engine::general_purpose::STANDARD
                    .encode(digest_bytes(&algorithm, data.as_bytes()).map_err(js_error)?))
            },
        )?,
    )?;
    Ok(object)
}
