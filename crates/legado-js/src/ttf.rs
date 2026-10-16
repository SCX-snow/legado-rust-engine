use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use legado_core::{CacheNamespace, Error};
use rquickjs::function::Rest;
use rquickjs::{Ctx, Function, Object, Value as JsValue};

use crate::host::Host;
use crate::java::js_err;

pub(crate) struct Font {
    pub unicode_to_glyph: HashMap<u32, Option<String>>,
    pub glyph_to_unicode: HashMap<String, u32>,
    pub unicode_to_glyph_id: HashMap<u32, u16>,
    pub glyphs: Vec<Option<String>>,
}

impl Font {
    pub fn get_glyf_id_by_unicode(&self, unicode: u32) -> u16 {
        self.unicode_to_glyph_id.get(&unicode).copied().unwrap_or(0)
    }

    pub fn get_glyf_by_unicode(&self, unicode: u32) -> Option<String> {
        self.unicode_to_glyph.get(&unicode).cloned().flatten()
    }

    pub fn get_glyf_by_id(&self, glyf_id: usize) -> Option<String> {
        self.glyphs.get(glyf_id).cloned().flatten()
    }

    pub fn get_unicode_by_glyf(&self, glyph: Option<&str>) -> u32 {
        let Some(glyph) = glyph else {
            return 0;
        };
        self.glyph_to_unicode.get(glyph).copied().unwrap_or(0)
    }

    pub fn is_blank_unicode(unicode: u32) -> bool {
        matches!(
            unicode,
            0x0009
                | 0x0020
                | 0x00A0
                | 0x2002
                | 0x2003
                | 0x2007
                | 0x200A
                | 0x200B
                | 0x200C
                | 0x200D
                | 0x202F
                | 0x205F
        )
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let directories = read_directory(bytes)?;
        let index_to_loc_format = read_head(bytes, &directories)?;
        let unicode_to_glyph_id = read_cmap(bytes, &directories)?;
        let loca = read_loca(bytes, &directories, index_to_loc_format);
        let max_contours = read_maxp(bytes, &directories)?;
        let glyphs = read_glyf(bytes, &directories, &loca, max_contours);

        let mut unicode_to_glyph: HashMap<u32, Option<String>> = HashMap::new();
        let mut glyph_to_unicode: HashMap<String, u32> = HashMap::new();
        let mut codes: Vec<u32> = unicode_to_glyph_id.keys().copied().collect();
        codes.sort_unstable();
        for code in codes {
            let glyf_id = unicode_to_glyph_id[&code] as usize;
            let Some(fingerprint) = glyphs.get(glyf_id).cloned().flatten() else {
                unicode_to_glyph.insert(code, None);
                continue;
            };
            glyph_to_unicode.insert(fingerprint.clone(), code);
            unicode_to_glyph.insert(code, Some(fingerprint));
        }
        Ok(Self {
            unicode_to_glyph,
            glyph_to_unicode,
            unicode_to_glyph_id,
            glyphs,
        })
    }
}

pub(crate) fn replace_font(text: &str, error: &Font, correct: &Font, filter: bool) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let code = ch as u32;
        if Font::is_blank_unicode(code) {
            out.push(ch);
            continue;
        }
        let mut glyf = error.get_glyf_by_unicode(code);
        if error.get_glyf_id_by_unicode(code) == 0 {
            glyf = None;
        }
        if filter && glyf.is_none() {
            continue;
        }
        let found = correct.get_unicode_by_glyf(glyf.as_deref());
        if found != 0 {
            if let Some(mapped) = char::from_u32(found & 0xFFFF) {
                out.push(mapped);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn read_directory(bytes: &[u8]) -> Result<HashMap<String, (usize, usize)>, String> {
    let mut r = Reader::new(bytes, 0);
    r.u32();
    let num_tables = r.u16();
    r.u16();
    r.u16();
    r.u16();
    let mut directories = HashMap::new();
    for _ in 0..num_tables {
        let tag = r.tag();
        let _check_sum = r.u32();
        let offset = r.u32() as usize;
        let length = r.u32() as usize;
        directories.insert(tag, (offset, length));
    }
    Ok(directories)
}

fn table<'a>(
    bytes: &'a [u8],
    directories: &HashMap<String, (usize, usize)>,
    tag: &str,
) -> Result<(&'a [u8], (usize, usize)), String> {
    let entry = directories
        .get(tag)
        .copied()
        .ok_or_else(|| format!("字体缺 {tag} 表"))?;
    Ok((bytes, entry))
}

fn read_head(bytes: &[u8], directories: &HashMap<String, (usize, usize)>) -> Result<i16, String> {
    let (_, (offset, _)) = table(bytes, directories, "head")?;
    let mut r = Reader::new(bytes, offset);
    r.u16();
    r.u16();
    r.u32();
    r.u32();
    r.u32();
    r.u16();
    r.u16();
    r.u64();
    r.u64();
    r.i16();
    r.i16();
    r.i16();
    r.i16();
    r.u16();
    r.u16();
    r.i16();
    Ok(r.i16())
}

fn read_cmap(
    bytes: &[u8],
    directories: &HashMap<String, (usize, usize)>,
) -> Result<HashMap<u32, u16>, String> {
    let (_, (offset, _)) = table(bytes, directories, "cmap")?;
    let mut r = Reader::new(bytes, offset);
    r.u16();
    let num_tables = r.u16();
    let mut records = Vec::new();
    for _ in 0..num_tables {
        let _platform = r.u16();
        let _encoding = r.u16();
        let sub_offset = r.u32() as usize;
        records.push(sub_offset);
    }

    let mut unicode_to_glyph_id: HashMap<u32, u16> = HashMap::new();
    let mut seen = BTreeSet::new();
    for sub_offset in records {
        if !seen.insert(sub_offset) {
            continue;
        }
        let mut f = Reader::new(bytes, offset + sub_offset);
        let format = f.u16();
        let length = f.u16() as usize;
        f.u16();
        match format {
            0 => {
                for code in 0..length.saturating_sub(6) {
                    let glyph_id = f.u8() as u16;
                    if glyph_id == 0 {
                        continue;
                    }
                    unicode_to_glyph_id.insert(code as u32, glyph_id);
                }
            }
            4 => {
                let seg_count_x2 = f.u16();
                let seg_count = (seg_count_x2 / 2) as usize;
                f.u16();
                f.u16();
                f.u16();
                let end_code: Vec<u16> = (0..seg_count).map(|_| f.u16()).collect();
                f.u16();
                let start_code: Vec<u16> = (0..seg_count).map(|_| f.u16()).collect();
                let id_delta: Vec<i16> = (0..seg_count).map(|_| f.i16()).collect();
                let id_range_offsets: Vec<u16> = (0..seg_count).map(|_| f.u16()).collect();
                let glyph_id_count = (length as i32 - 16 - (seg_count as i32 * 8)) / 2;
                let glyph_id_count = glyph_id_count.max(0) as usize;
                let glyph_id_array: Vec<u16> = (0..glyph_id_count).map(|_| f.u16()).collect();

                for segment in 0..seg_count {
                    let start = start_code[segment] as i32;
                    let end = end_code[segment] as i32;
                    let delta = id_delta[segment] as i32;
                    let range_offset = id_range_offsets[segment] as i32;
                    for code in start..=end {
                        let glyph_id = if range_offset == 0 {
                            (code + delta) & 0xFFFF
                        } else {
                            let index = (range_offset / 2) + (code - start) + segment as i32
                                - seg_count as i32;
                            if index >= 0 && (index as usize) < glyph_id_count {
                                glyph_id_array[index as usize] as i32 + delta
                            } else {
                                0
                            }
                        };
                        if glyph_id == 0 {
                            continue;
                        }
                        unicode_to_glyph_id.insert(code as u32, (glyph_id & 0xFFFF) as u16);
                    }
                }
            }
            6 => {
                let first_code = f.u16();
                let entry_count = f.u16();
                for index in 0..entry_count {
                    let glyph_id = f.u16();
                    unicode_to_glyph_id.insert(first_code as u32 + index as u32, glyph_id);
                }
            }
            _ => {}
        }
    }
    Ok(unicode_to_glyph_id)
}

fn read_loca(
    bytes: &[u8],
    directories: &HashMap<String, (usize, usize)>,
    index_to_loc_format: i16,
) -> Vec<u32> {
    let Ok((_, (offset, length))) = table(bytes, directories, "loca") else {
        return Vec::new();
    };
    let mut loca = Vec::new();
    if index_to_loc_format == 0 {
        for index in 0..length / 2 {
            loca.push(Reader::new(bytes, offset + index * 2).u16() as u32 * 2);
        }
    } else {
        for index in 0..length / 4 {
            loca.push(Reader::new(bytes, offset + index * 4).u32());
        }
    }
    loca
}

fn read_maxp(bytes: &[u8], directories: &HashMap<String, (usize, usize)>) -> Result<i16, String> {
    let (_, (offset, _)) = table(bytes, directories, "maxp")?;
    let mut r = Reader::new(bytes, offset);
    r.u32();
    r.u16();
    r.u16();
    Ok(r.u16() as i16)
}

fn read_glyf(
    bytes: &[u8],
    directories: &HashMap<String, (usize, usize)>,
    loca: &[u32],
    max_contours: i16,
) -> Vec<Option<String>> {
    let Ok((_, (offset, _))) = table(bytes, directories, "glyf") else {
        return Vec::new();
    };
    let num_glyphs = loca.len().saturating_sub(1);
    let mut glyphs = vec![None; num_glyphs];
    for index in 0..num_glyphs {
        let (start, end) = (loca[index], loca[index + 1]);
        if start == end {
            continue;
        }
        let mut r = Reader::new(bytes, offset + start as usize);
        let number_of_contours = r.i16();
        if number_of_contours as i32 > max_contours as i32 {
            continue;
        }
        r.i16();
        r.i16();
        r.i16();
        r.i16();
        if number_of_contours == 0 {
            continue;
        }
        glyphs[index] = Some(if number_of_contours > 0 {
            read_simple_glyph(&mut r, number_of_contours as usize)
        } else {
            read_composite_glyph(&mut r)
        });
    }
    glyphs
}

fn read_simple_glyph(r: &mut Reader<'_>, contours: usize) -> String {
    let end_pts: Vec<u16> = (0..contours).map(|_| r.u16()).collect();
    let instruction_length = r.u16() as usize;
    for _ in 0..instruction_length {
        r.u8();
    }
    let flag_length = end_pts.last().copied().unwrap_or(0) as usize + 1;
    let mut flags = vec![0u8; flag_length];
    let mut index = 0;
    while index < flag_length {
        let flag = r.u8();
        flags[index] = flag;
        if flag & 0x08 == 0x08 {
            let mut repeat = r.u8();
            while repeat > 0 && index + 1 < flag_length {
                index += 1;
                flags[index] = flag;
                repeat -= 1;
            }
        }
        index += 1;
    }
    let mut x = vec![0i32; flag_length];
    for index in 0..flag_length {
        x[index] = match flags[index] & 0x12 {
            0x02 => -(r.u8() as i32),
            0x12 => r.u8() as i32,
            0x10 => 0,
            _ => r.i16() as i32,
        };
    }
    let mut y = vec![0i32; flag_length];
    for index in 0..flag_length {
        y[index] = match flags[index] & 0x24 {
            0x04 => -(r.u8() as i32),
            0x24 => r.u8() as i32,
            0x20 => 0,
            _ => r.i16() as i32,
        };
    }
    x.iter()
        .zip(y.iter())
        .map(|(x, y)| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn read_composite_glyph(r: &mut Reader<'_>) -> String {
    let mut parts = Vec::new();
    loop {
        let flags = r.u16();
        let glyph_index = r.u16();
        let (arg1, arg2) = match flags & 0b11 {
            0b00 => (r.u8() as i32, r.u8() as i32),
            0b10 => (r.i8() as i32, r.i8() as i32),
            0b01 => (r.u16() as i32, r.u16() as i32),
            _ => (r.i16() as i32, r.i16() as i32),
        };
        let (mut x_scale, mut scale01, mut scale10, mut y_scale) = (0f32, 0f32, 0f32, 0f32);
        match flags & 0b1100_1000 {
            0b0000_1000 => {
                let scale = r.u16() as f32 / 16384.0;
                x_scale = scale;
                y_scale = scale;
            }
            0b0100_0000 => {
                x_scale = r.u16() as f32 / 16384.0;
                y_scale = r.u16() as f32 / 16384.0;
            }
            0b1000_0000 => {
                x_scale = r.u16() as f32 / 16384.0;
                scale01 = r.u16() as f32 / 16384.0;
                scale10 = r.u16() as f32 / 16384.0;
                y_scale = r.u16() as f32 / 16384.0;
            }
            _ => {}
        }
        parts.push(format!(
            "{{flags:{flags},glyphIndex:{glyph_index},arg1:{arg1},arg2:{arg2},\
             xScale:{x_scale:?},scale01:{scale01:?},scale10:{scale10:?},yScale:{y_scale:?}}}"
        ));
        if flags & 0x20 == 0 {
            break;
        }
    }
    format!("[{}]", parts.join(","))
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], pos: usize) -> Self {
        Self { bytes, pos }
    }

    fn u8(&mut self) -> u8 {
        let value = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        value
    }

    fn i8(&mut self) -> i8 {
        self.u8() as i8
    }

    fn u16(&mut self) -> u16 {
        let hi = self.u8() as u16;
        let lo = self.u8() as u16;
        (hi << 8) | lo
    }

    fn i16(&mut self) -> i16 {
        self.u16() as i16
    }

    fn u32(&mut self) -> u32 {
        let mut value = 0u32;
        for _ in 0..4 {
            value = (value << 8) | self.u8() as u32;
        }
        value
    }

    fn u64(&mut self) {
        for _ in 0..8 {
            self.u8();
        }
    }

    fn tag(&mut self) -> String {
        let start = self.pos;
        for _ in 0..4 {
            self.u8();
        }
        String::from_utf8_lossy(&self.bytes[start..self.pos.min(self.bytes.len())]).into_owned()
    }
}

pub(crate) fn query_ttf<'js>(ctx: &Ctx<'js>, host: &Rc<Host>) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, args: Rest<JsValue<'js>>| -> rquickjs::Result<JsValue<'js>> {
            let use_cache = match args.0.get(1) {
                Some(value) if value.is_bool() => value.as_bool().unwrap_or(true),
                _ => true,
            };
            let font = query_font(&host, args.0.first(), use_cache).map_err(js_err)?;
            match font {
                Some(font) => Ok(font_object(&ctx, &host, font)?.into_value()),
                None => Ok(JsValue::new_null(ctx.clone())),
            }
        },
    )
}

pub(crate) fn replace_font_fn<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(
        ctx.clone(),
        move |args: Rest<JsValue<'js>>| -> rquickjs::Result<String> {
            let text = args.0.first().map(crate::java::js_text).unwrap_or_default();
            let filter = match args.0.get(3) {
                Some(value) if value.is_bool() => value.as_bool().unwrap_or(false),
                _ => false,
            };
            let error = font_of(&host, args.0.get(1));
            let correct = font_of(&host, args.0.get(2));
            let (Some(error), Some(correct)) = (error, correct) else {
                return Ok(text);
            };
            Ok(replace_font(&text, &error, &correct, filter))
        },
    )
}

fn font_of(host: &Rc<Host>, value: Option<&JsValue<'_>>) -> Option<Rc<Font>> {
    let value = value?;
    let object = value.as_object()?;
    let id: i32 = object.get("__ttf").ok()?;
    host.font(id as usize)
}

fn font_object<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    font: Rc<Font>,
) -> rquickjs::Result<Object<'js>> {
    let object = Object::new(ctx.clone())?;
    let id = host.register_font(font);
    object.prop("__ttf", id)?;
    object.prop("getGlyfById", glyph_by_id(ctx, host, id)?)?;
    object.prop("getGlyfIdByUnicode", glyph_id_by_unicode(ctx, host, id)?)?;
    object.prop("getGlyfByUnicode", glyph_by_unicode(ctx, host, id)?)?;
    object.prop("getUnicodeByGlyf", unicode_by_glyph(ctx, host, id)?)?;
    object.prop("isBlankUnicode", is_blank_unicode(ctx)?)?;
    Ok(object)
}

fn glyph_by_id<'js>(ctx: &Ctx<'js>, host: &Rc<Host>, id: usize) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    let inner = ctx.clone();
    Function::new(
        ctx.clone(),
        move |glyf_id: i32| -> rquickjs::Result<JsValue<'js>> {
            Ok(
                match host
                    .font(id)
                    .and_then(|font| font.get_glyf_by_id(glyf_id.max(0) as usize))
                {
                    Some(text) => rquickjs::String::from_str(inner.clone(), &text)?.into_value(),
                    None => JsValue::new_null(inner.clone()),
                },
            )
        },
    )
}

fn glyph_id_by_unicode<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    id: usize,
) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(ctx.clone(), move |unicode: i32| -> u16 {
        host.font(id)
            .map(|font| font.get_glyf_id_by_unicode(unicode as u32))
            .unwrap_or(0)
    })
}

fn glyph_by_unicode<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    id: usize,
) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    let inner = ctx.clone();
    Function::new(
        ctx.clone(),
        move |unicode: i32| -> rquickjs::Result<JsValue<'js>> {
            Ok(
                match host
                    .font(id)
                    .and_then(|font| font.get_glyf_by_unicode(unicode as u32))
                {
                    Some(text) => rquickjs::String::from_str(inner.clone(), &text)?.into_value(),
                    None => JsValue::new_null(inner.clone()),
                },
            )
        },
    )
}

fn unicode_by_glyph<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<Host>,
    id: usize,
) -> rquickjs::Result<Function<'js>> {
    let host = host.clone();
    Function::new(ctx.clone(), move |glyph: Option<String>| -> u32 {
        host.font(id)
            .map(|font| font.get_unicode_by_glyf(glyph.as_deref()))
            .unwrap_or(0)
    })
}

fn is_blank_unicode<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), move |unicode: i32| -> bool {
        Font::is_blank_unicode(unicode as u32)
    })
}

fn query_font(
    host: &Rc<Host>,
    value: Option<&JsValue<'_>>,
    use_cache: bool,
) -> Result<Option<Rc<Font>>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let (cache_key, source) = if let Some(text) = value.as_string() {
        let text = text
            .to_string()
            .map_err(|error| Error::Js(format!("queryTTF 参数不是字符串：{error}")))?;
        let key = use_cache.then(|| font_cache_key(sha256(&text)));
        if is_abs_url(&text) {
            (key, Source::Url(text))
        } else if text.starts_with("content://") || text.starts_with("/storage") {
            return Err(Error::Js("queryTTF 暂不支持本地文件路径".into()));
        } else {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text.as_bytes())
                .map_err(|error| Error::Js(format!("queryTTF 的 base64 解析失败：{error}")))?;
            (key, Source::Bytes(bytes))
        }
    } else if let Some(array) = value.as_array() {
        let mut bytes = Vec::with_capacity(array.len());
        for item in array.iter::<i32>() {
            let item =
                item.map_err(|error| Error::Js(format!("queryTTF 字节数组非法：{error}")))?;
            bytes.push(item as u8);
        }
        let key = use_cache.then(|| font_cache_key(sha256_bytes(&bytes)));
        (key, Source::Bytes(bytes))
    } else {
        return Ok(None);
    };

    if let Some(key) = &cache_key
        && let Some(store) = host.cache.as_ref()
        && let Ok(Some(cached)) = host.runtime.block_on(store.get(key))
    {
        let font = Font::parse(&cached).map_err(Error::Js)?;
        return Ok(Some(Rc::new(font)));
    }

    let bytes = match source {
        Source::Url(url) => fetch_bytes(host, &url)?,
        Source::Bytes(bytes) => bytes,
    };
    let font = Font::parse(&bytes).map_err(Error::Js)?;
    if let Some(key) = &cache_key
        && let Some(store) = host.cache.as_ref()
    {
        let _ = host
            .runtime
            .block_on(store.put(key, bytes.as_slice().to_vec().into(), None));
    }
    Ok(Some(Rc::new(font)))
}

fn font_cache_key(hash: String) -> String {
    CacheNamespace::QueryTtf.key(&hash)
}

enum Source {
    Url(String),
    Bytes(Vec<u8>),
}

fn is_abs_url(text: &str) -> bool {
    text.starts_with("http://") || text.starts_with("https://")
}

fn fetch_bytes(host: &Rc<Host>, url: &str) -> Result<Vec<u8>, Error> {
    let Some(fetcher) = host.http.as_ref() else {
        return Err(Error::Js(
            "没有接 HttpFetcher，queryTTF 下载不了字体".into(),
        ));
    };
    let request = legado_core::HttpRequest::get(url);
    let response = host
        .runtime
        .block_on(fetcher.fetch(request))
        .map_err(|error| Error::Js(format!("queryTTF 下载失败：{error}")))?;
    Ok(response.body.to_vec())
}

fn sha256(text: &str) -> String {
    sha256_bytes(text.as_bytes())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
