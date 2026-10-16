use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::function::Opt;
use rquickjs::{Ctx, Function, Object, Value as JsValue};

use crate::crypto::{self, Algorithm, Mode, Padding, Symmetric, hmac_bytes, parse_transformation};
use crate::java::{bytes_to_js, decode_base64, js_bytes, js_text};

fn js_error(message: String) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("宿主", "javax", message)
}

pub(crate) fn mac<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set(
        "getInstance",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, algorithm: String| {
            crypto::check_hmac_algorithm(&algorithm)
                .map(|()| mac_object(&ctx, algorithm.clone()))
                .map_err(js_error)
        })?,
    )?;
    Ok(object)
}

#[derive(Default)]
struct MacState {
    key: Option<Vec<u8>>,
    buffer: Vec<u8>,
}

fn mac_object<'js>(ctx: &Ctx<'js>, algorithm: String) -> Object<'js> {
    let state: Rc<RefCell<MacState>> = Rc::new(RefCell::new(MacState::default()));
    let object = Object::new(ctx.clone()).expect("建 Mac");
    object
        .set(
            "init",
            Function::new(ctx.clone(), {
                let state = state.clone();
                let object = object.clone();
                move |key: JsValue<'js>| -> Object<'js> {
                    state.borrow_mut().key = Some(key_bytes(&key));
                    object.clone()
                }
            })
            .expect("建 init"),
        )
        .expect("挂 init");
    object
        .set(
            "update",
            Function::new(ctx.clone(), {
                let state = state.clone();
                move |data: JsValue<'js>| {
                    state.borrow_mut().buffer.extend(js_bytes(&data));
                }
            })
            .expect("建 update"),
        )
        .expect("挂 update");
    object
        .set(
            "doFinal",
            Function::new(ctx.clone(), {
                let state = state.clone();
                move |ctx: Ctx<'js>, data: Opt<JsValue<'js>>| -> rquickjs::Result<JsValue<'js>> {
                    let mut state = state.borrow_mut();
                    let Some(key) = state.key.clone() else {
                        return Err(js_error(
                            "Mac 还没 init（要先用 SecretKeySpec 设密钥）".into(),
                        ));
                    };
                    let mut input = std::mem::take(&mut state.buffer);
                    if let Some(data) = data.0 {
                        input.extend(js_bytes(&data));
                    }
                    drop(state);
                    let out = hmac_bytes(&algorithm, &key, &input).map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &out))
                }
            })
            .expect("建 doFinal"),
        )
        .expect("挂 doFinal");
    object
}

pub(crate) fn cipher<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    object.set("ENCRYPT_MODE", 1)?;
    object.set("DECRYPT_MODE", 2)?;
    object.set(
        "getInstance",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, transformation: String| {
            parse_transformation(&transformation)
                .map(|parsed| cipher_object(&ctx, parsed))
                .map_err(js_error)
        })?,
    )?;
    Ok(object)
}

type Parsed = (Algorithm, Mode, Padding);

fn cipher_object<'js>(ctx: &Ctx<'js>, parsed: Parsed) -> Object<'js> {
    let state: Rc<RefCell<Option<(Symmetric, bool)>>> = Rc::new(RefCell::new(None));
    let object = Object::new(ctx.clone()).expect("建 Cipher");
    object
        .set(
            "init",
            Function::new(ctx.clone(), {
                let state = state.clone();
                let object = object.clone();
                move |mode: i32,
                      key: JsValue<'js>,
                      iv: Opt<JsValue<'js>>|
                      -> rquickjs::Result<Object<'js>> {
                    let iv = iv.0.as_ref().map(key_bytes).unwrap_or_default();
                    let symmetric =
                        Symmetric::new(parsed.0, parsed.1, parsed.2, key_bytes(&key), iv)
                            .map_err(js_error)?;
                    *state.borrow_mut() = Some((symmetric, mode != 2));
                    Ok(object.clone())
                }
            })
            .expect("建 init"),
        )
        .expect("挂 init");
    object
        .set(
            "doFinal",
            Function::new(ctx.clone(), {
                let state = state.clone();
                move |ctx: Ctx<'js>, data: JsValue<'js>| -> rquickjs::Result<JsValue<'js>> {
                    let state = state.borrow();
                    let Some((symmetric, encrypt)) = state.as_ref() else {
                        return Err(js_error("Cipher 还没 init".into()));
                    };
                    let input = if *encrypt {
                        js_bytes(&data)
                    } else {
                        decode_input(&data).map_err(js_error)?
                    };
                    let out = if *encrypt {
                        symmetric.encrypt(&input)
                    } else {
                        symmetric.decrypt(&input)
                    }
                    .map_err(js_error)?;
                    Ok(bytes_to_js(&ctx, &out))
                }
            })
            .expect("建 doFinal"),
        )
        .expect("挂 doFinal");
    object
}

pub(crate) fn key_spec<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         key: JsValue<'js>,
         _algorithm: Opt<String>|
         -> rquickjs::Result<Object<'js>> {
            let object = Object::new(ctx.clone())?;
            object.set("__legado_bytes", bytes_to_js(&ctx, &key_bytes(&key)))?;
            object.set(
                "getEncoded",
                Function::new(ctx.clone(), {
                    let bytes = key_bytes(&key);
                    move |ctx: Ctx<'js>| -> JsValue<'js> { bytes_to_js(&ctx, &bytes) }
                })?,
            )?;
            object.set("getAlgorithm", _algorithm.0.unwrap_or_default())?;
            Ok(object)
        },
    )
}

pub(crate) fn datatype_converter<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Object<'js>> {
    use base64::Engine as _;
    let object = Object::new(ctx.clone())?;
    object.set(
        "printBase64Binary",
        Function::new(ctx.clone(), |data: JsValue<'_>| -> String {
            base64::engine::general_purpose::STANDARD.encode(js_bytes(&data))
        })?,
    )?;
    object.set(
        "parseBase64Binary",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, text: String| -> rquickjs::Result<JsValue<'js>> {
                Ok(bytes_to_js(&ctx, &decode_base64(&text).unwrap_or_default()))
            },
        )?,
    )?;
    object.set(
        "printHexBinary",
        Function::new(ctx.clone(), |data: JsValue<'_>| -> String {
            hex::encode(js_bytes(&data))
        })?,
    )?;
    object.set(
        "parseHexBinary",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, text: String| -> rquickjs::Result<JsValue<'js>> {
                Ok(bytes_to_js(
                    &ctx,
                    &hex::decode(text.trim()).unwrap_or_default(),
                ))
            },
        )?,
    )?;
    Ok(object)
}

fn key_bytes(value: &JsValue<'_>) -> Vec<u8> {
    if let Some(object) = value.as_object()
        && let Ok(stored) = object.get::<_, JsValue<'_>>("__legado_bytes")
        && stored.as_array().is_some()
    {
        return js_bytes(&stored);
    }
    js_bytes(value)
}

fn decode_input(value: &JsValue<'_>) -> Result<Vec<u8>, String> {
    if value.as_array().is_some() {
        return Ok(js_bytes(value));
    }
    let text = js_text(value);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    crate::crypto::decode_any(&text)
}
