use std::sync::LazyLock;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Zh,
    En,
}

static LANG: LazyLock<Lang> = LazyLock::new(detect_lang);

fn detect_lang() -> Lang {
    for key in ["LC_ALL", "LC_TIME", "LANG"] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let language = value
            .split(['.', '@'])
            .next()
            .unwrap_or_default()
            .split(['_', '-'])
            .next()
            .unwrap_or_default();
        return if language.eq_ignore_ascii_case("zh") {
            Lang::Zh
        } else {
            Lang::En
        };
    }
    Lang::En
}

pub(crate) fn format(millis: i64, offset_millis: i32, pattern: &str) -> Result<String, String> {
    let Some(wall) = Utc
        .timestamp_millis_opt(millis + i64::from(offset_millis))
        .single()
    else {
        return Ok(String::new());
    };
    let zone_seconds = offset_millis / 1000;
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut quoted = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\'' {
            if chars.get(index + 1) == Some(&'\'') {
                out.push('\'');
                index += 2;
                continue;
            }
            quoted = !quoted;
            index += 1;
            continue;
        }
        if quoted || !ch.is_ascii_alphabetic() {
            out.push(ch);
            index += 1;
            continue;
        }
        let count = chars[index..]
            .iter()
            .take_while(|next| **next == ch)
            .count();
        write_field(&mut out, &wall, zone_seconds, ch, count, *LANG)?;
        index += count;
    }
    Ok(out)
}

fn write_field(
    out: &mut String,
    wall: &DateTime<Utc>,
    zone_seconds: i32,
    letter: char,
    count: usize,
    lang: Lang,
) -> Result<(), String> {
    let (era_ad, year) = if wall.year() > 0 {
        (true, wall.year() as u32)
    } else {
        (false, (1 - wall.year()) as u32)
    };
    match letter {
        'G' => out.push_str(era(lang, era_ad)),
        'y' => push_year(out, year, count),
        'Y' => {
            let (week_year, _) = week_of_year(wall);
            push_year(out, week_year.unsigned_abs(), count);
        }
        'M' | 'L' => push_month(out, wall.month(), count, lang),
        'w' => {
            let (_, week) = week_of_year(wall);
            out.push_str(&digits(week, count));
        }
        'W' => out.push_str(&digits(week_of_month(wall), count)),
        'D' => out.push_str(&digits(wall.ordinal(), count)),
        'd' => out.push_str(&digits(wall.day(), count)),
        'F' => out.push_str(&digits(day_of_week_in_month(wall), count)),
        'E' => {
            let day = wall.weekday().num_days_from_monday() as usize;
            out.push_str(weekday_name(lang, day, count <= 3));
        }
        'u' => out.push_str(&digits(wall.weekday().num_days_from_monday() + 1, count)),
        'a' => out.push_str(ampm(lang, wall.hour() >= 12)),
        'H' => out.push_str(&digits(wall.hour(), count)),
        'k' => out.push_str(&digits(
            if wall.hour() == 0 { 24 } else { wall.hour() },
            count,
        )),
        'K' => out.push_str(&digits(wall.hour() % 12, count)),
        'h' => {
            let hour = wall.hour() % 12;
            out.push_str(&digits(if hour == 0 { 12 } else { hour }, count));
        }
        'm' => out.push_str(&digits(wall.minute(), count)),
        's' => out.push_str(&digits(wall.second(), count)),
        'S' => out.push_str(&digits(wall.timestamp_subsec_millis(), count)),
        'z' => out.push_str(if count < 4 {
            "UTC"
        } else {
            zone_long_name(lang)
        }),
        'Z' => out.push_str(&rfc822(zone_seconds)),
        'X' => {
            if count > 3 {
                return Err(format!("invalid ISO 8601 format: length={count}"));
            }
            out.push_str(&iso8601(zone_seconds, count));
        }
        _ => {
            for _ in 0..count {
                out.push(letter);
            }
        }
    }
    Ok(())
}

fn digits(value: u32, count: usize) -> String {
    let text = value.to_string();
    if text.len() >= count {
        text
    } else {
        format!("{}{}", "0".repeat(count - text.len()), text)
    }
}

fn push_year(out: &mut String, year: u32, count: usize) {
    if count == 2 {
        out.push_str(&digits(year % 100, 2));
    } else {
        out.push_str(&digits(year, count));
    }
}

fn push_month(out: &mut String, month: u32, count: usize, lang: Lang) {
    match count {
        1 => out.push_str(&digits(month, 1)),
        2 => out.push_str(&digits(month, 2)),
        3 => out.push_str(month_name(lang, month, MonthStyle::Short)),
        4 => out.push_str(month_name(lang, month, MonthStyle::Full)),
        _ => out.push_str(month_name(lang, month, MonthStyle::Narrow)),
    }
}

fn day_of_week_in_month(wall: &DateTime<Utc>) -> u32 {
    (wall.day() - 1) / 7 + 1
}

fn week_of_month(wall: &DateTime<Utc>) -> u32 {
    let Some(first) = NaiveDate::from_ymd_opt(wall.year(), wall.month(), 1) else {
        return 1;
    };
    let offset = first.weekday().num_days_from_sunday();
    (wall.day() - 1 + offset) / 7 + 1
}

fn week_of_year(wall: &DateTime<Utc>) -> (i32, u32) {
    let year = wall.year();
    let Some(jan1) = NaiveDate::from_ymd_opt(year, 1, 1) else {
        return (year, 1);
    };
    let ordinal = i64::from(wall.ordinal());
    let offset = i64::from(jan1.weekday().num_days_from_sunday());
    let days_in_year = i64::from(
        NaiveDate::from_ymd_opt(year, 12, 31)
            .map(|last| last.ordinal())
            .unwrap_or(365),
    );
    let mut week = (ordinal - 1 + offset) / 7 + 1;
    let mut week_year = year;
    if let Some(next_jan1) = NaiveDate::from_ymd_opt(year + 1, 1, 1) {
        let next_offset = i64::from(next_jan1.weekday().num_days_from_sunday());
        let next_start = days_in_year + 1 - next_offset;
        if ordinal >= next_start {
            week = (ordinal - next_start) / 7 + 1;
            week_year = year + 1;
        }
    }
    (week_year, week as u32)
}

fn rfc822(zone_seconds: i32) -> String {
    let sign = if zone_seconds < 0 { '-' } else { '+' };
    let abs = zone_seconds.unsigned_abs();
    format!("{sign}{:02}{:02}", abs / 3600, (abs % 3600) / 60)
}

fn iso8601(zone_seconds: i32, count: usize) -> String {
    if zone_seconds == 0 {
        return "Z".to_string();
    }
    let sign = if zone_seconds < 0 { '-' } else { '+' };
    let abs = zone_seconds.unsigned_abs();
    let (hours, minutes) = (abs / 3600, (abs % 3600) / 60);
    match count {
        1 => format!("{sign}{hours:02}"),
        2 => format!("{sign}{hours:02}{minutes:02}"),
        _ => format!("{sign}{hours:02}:{minutes:02}"),
    }
}

enum MonthStyle {
    Short,
    Full,
    Narrow,
}

fn month_name(lang: Lang, month: u32, style: MonthStyle) -> &'static str {
    let index = (month.clamp(1, 12) - 1) as usize;
    match lang {
        Lang::Zh => match style {
            MonthStyle::Short => ZH_MONTHS_SHORT[index],
            _ => ZH_MONTHS[index],
        },
        Lang::En => match style {
            MonthStyle::Short => EN_MONTHS_SHORT[index],
            MonthStyle::Full => EN_MONTHS[index],
            MonthStyle::Narrow => EN_MONTHS_NARROW[index],
        },
    }
}

fn weekday_name(lang: Lang, day: usize, short: bool) -> &'static str {
    match lang {
        Lang::Zh => {
            if short {
                ZH_WEEKDAYS_SHORT[day]
            } else {
                ZH_WEEKDAYS[day]
            }
        }
        Lang::En => {
            if short {
                EN_WEEKDAYS_SHORT[day]
            } else {
                EN_WEEKDAYS[day]
            }
        }
    }
}

fn ampm(lang: Lang, pm: bool) -> &'static str {
    match (lang, pm) {
        (Lang::Zh, false) => "上午",
        (Lang::Zh, true) => "下午",
        (Lang::En, false) => "AM",
        (Lang::En, true) => "PM",
    }
}

fn era(lang: Lang, ad: bool) -> &'static str {
    match (lang, ad) {
        (Lang::Zh, true) => "公元",
        (Lang::Zh, false) => "公元前",
        (Lang::En, true) => "AD",
        (Lang::En, false) => "BC",
    }
}

fn zone_long_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "协调世界时",
        Lang::En => "Coordinated Universal Time",
    }
}

const ZH_MONTHS: [&str; 12] = [
    "一月",
    "二月",
    "三月",
    "四月",
    "五月",
    "六月",
    "七月",
    "八月",
    "九月",
    "十月",
    "十一月",
    "十二月",
];
const ZH_MONTHS_SHORT: [&str; 12] = [
    "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
];
const ZH_WEEKDAYS: [&str; 7] = [
    "星期一",
    "星期二",
    "星期三",
    "星期四",
    "星期五",
    "星期六",
    "星期日",
];
const ZH_WEEKDAYS_SHORT: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

const EN_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const EN_MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const EN_MONTHS_NARROW: [&str; 12] = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"];
const EN_WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const EN_WEEKDAYS_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
