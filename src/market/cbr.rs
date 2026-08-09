//! CBR DailyInfo SOAP + legacy XML_daily fallback.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::str::FromStr;

use super::upsert_cbr_indicators;
use crate::fx;
use sqlx::PgPool;

#[derive(Debug, Default, Clone)]
pub struct CbrBundle {
    pub key_rate: Option<Decimal>,
    pub key_rate_date: Option<NaiveDate>,
    pub inflation: Option<Decimal>,
    pub inflation_date: Option<NaiveDate>,
    pub usd_rub: Option<Decimal>,
    pub eur_rub: Option<Decimal>,
    pub cny_rub: Option<Decimal>,
    pub fx_as_of: Option<NaiveDate>,
    pub gold_rub: Option<Decimal>,
    pub reserves_bln_usd: Option<Decimal>,
    pub ruonia: Option<Decimal>,
    pub raw_main: String,
    pub raw_all: String,
}

fn soap_envelope(body_inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<soap:Envelope xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
 xmlns:xsd="http://www.w3.org/2001/XMLSchema"
 xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">
  <soap:Body>{body_inner}</soap:Body>
</soap:Envelope>"#
    )
}

async fn soap_post(url: &str, action: &str, body_inner: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", action)
        .body(soap_envelope(body_inner))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.text().await.map_err(|e| e.to_string())
}

fn parse_ru_date(s: &str) -> Option<NaiveDate> {
    // DD.MM.YYYY
    let parts: Vec<_> = s.trim().split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    NaiveDate::from_ymd_opt(
        parts[2].parse().ok()?,
        parts[1].parse().ok()?,
        parts[0].parse().ok()?,
    )
}

fn tag_attr_val(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    let idx = xml.find(&open)?;
    let slice = &xml[idx..];
    let end = slice.find('>')?;
    let attrs = &slice[..end];
    let key = format!("{attr}=\"");
    let a = attrs.find(&key)? + key.len();
    let b = attrs[a..].find('"')? + a;
    Some(attrs[a..b].to_string())
}

fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let idx = xml.find(&open)?;
    let after = &xml[idx..];
    let gt = after.find('>')? + 1;
    let close = format!("</{tag}>");
    let end = after[gt..].find(&close)? + gt;
    Some(after[gt..end].trim().to_string())
}

fn dec(s: &str) -> Option<Decimal> {
    Decimal::from_str(s.trim().replace(',', ".").as_str()).ok()
}

pub async fn fetch_main_info(soap_url: &str) -> Result<String, String> {
    soap_post(
        soap_url,
        "http://web.cbr.ru/MainInfoXML",
        r#"<MainInfoXML xmlns="http://web.cbr.ru/" />"#,
    )
    .await
}

pub async fn fetch_all_data(soap_url: &str) -> Result<String, String> {
    soap_post(
        soap_url,
        "http://web.cbr.ru/AllDataInfoXML",
        r#"<AllDataInfoXML xmlns="http://web.cbr.ru/" />"#,
    )
    .await
}

pub fn parse_main_info(xml: &str) -> CbrBundle {
    let mut b = CbrBundle {
        raw_main: xml.to_string(),
        ..Default::default()
    };
    if let Some(v) = tag_text(xml, "keyRate") {
        b.key_rate = dec(&v);
        b.key_rate_date = tag_attr_val(xml, "keyRate", "Date").and_then(|d| parse_ru_date(&d));
    }
    if let Some(v) = tag_text(xml, "Inflation") {
        b.inflation = dec(&v);
        b.inflation_date = tag_attr_val(xml, "Inflation", "Date").and_then(|d| parse_ru_date(&d));
    }
    if let Some(v) = tag_text(xml, "GoldBaks") {
        b.reserves_bln_usd = dec(&v);
    }
    b
}

pub fn parse_all_data(xml: &str, mut b: CbrBundle) -> CbrBundle {
    b.raw_all = xml.to_string();
    if let Some(usd_block) = xml.find("<USD ").map(|i| &xml[i..]) {
        if let Some(c) = tag_text(usd_block, "curs") {
            b.usd_rub = dec(&c);
        }
        b.fx_as_of = tag_attr_val(usd_block, "USD", "OnDate").and_then(|d| parse_ru_date(&d));
    }
    if let Some(i) = xml.find("<EUR ") {
        if let Some(c) = tag_text(&xml[i..], "curs") {
            b.eur_rub = dec(&c);
        }
    }
    if let Some(i) = xml.find("<CNY ") {
        if let Some(c) = tag_text(&xml[i..], "curs") {
            b.cny_rub = dec(&c);
        }
    }
    // KEY_RATE attribute form
    if b.key_rate.is_none() {
        if let Some(v) = tag_attr_val(xml, "KEY_RATE", "val") {
            b.key_rate = dec(&v);
            b.key_rate_date = tag_attr_val(xml, "KEY_RATE", "date").and_then(|d| parse_ru_date(&d));
        }
    }
    if b.inflation.is_none() {
        if let Some(v) = tag_attr_val(xml, "Inflation", "val") {
            b.inflation = dec(&v);
            b.inflation_date =
                tag_attr_val(xml, "Inflation", "OnDate").and_then(|d| parse_ru_date(&d));
        }
    }
    if let Some(v) = tag_attr_val(xml, "Золото", "val").or_else(|| {
        // Cyrillic tag may appear as-is
        None
    }) {
        b.gold_rub = dec(&v);
    }
    // RUONIA D1
    if let Some(i) = xml.find("<RUONIA ") {
        let slice = &xml[i..];
        if let Some(d1) = slice.find("<D1 ").map(|j| &slice[j..]) {
            if let Some(v) = tag_attr_val(d1, "D1", "val") {
                b.ruonia = dec(&v);
            }
        }
    }
    b
}

pub async fn refresh_cbr_bundle(pool: &PgPool, soap_url: &str, xml_daily_url: &str) -> Result<(), String> {
    let main = fetch_main_info(soap_url).await.unwrap_or_default();
    let all = fetch_all_data(soap_url).await.unwrap_or_default();
    let mut bundle = parse_main_info(&main);
    if !all.is_empty() {
        bundle = parse_all_data(&all, bundle);
    }

    // Always refresh official FX table via XML_daily (existing path).
    let n = fx::refresh_cbr(pool, xml_daily_url)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(rates = n, "CBR XML_daily rates upserted");

    // If SOAP missed USD, pull from fx_rates just written.
    if bundle.usd_rub.is_none() {
        if let Ok(Some(mid)) = sqlx::query_scalar::<_, Decimal>(
            "SELECT mid FROM fx_rates WHERE base = 'USD' AND quote = 'RUB' ORDER BY as_of DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        {
            bundle.usd_rub = Some(mid);
        }
    }

    let raw = serde_json::json!({
        "main": bundle.raw_main.chars().take(4000).collect::<String>(),
        "all": bundle.raw_all.chars().take(8000).collect::<String>(),
    });

    upsert_cbr_indicators(
        pool,
        bundle.key_rate,
        bundle.key_rate_date,
        bundle.inflation,
        bundle.inflation_date,
        bundle.usd_rub,
        bundle.eur_rub,
        bundle.cny_rub,
        bundle.fx_as_of,
        bundle.gold_rub,
        bundle.reserves_bln_usd,
        bundle.ruonia,
        raw,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}
