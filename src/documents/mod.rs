//! Bank PDF documents — branded letterhead + TSE-inspired DSM seals.

mod dsm;

pub use dsm::{
    receipt_process_data, statement_process_data, DocumentSeal, DocumentSecurityModule,
    VerifyResult,
};

use std::f32::consts::TAU;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use printpdf::path::{PaintMode, WindingOrder};
use printpdf::*;
use qrcode::QrCode;

use crate::ledger::{AccountView, TxnDetail, TxnView};

#[derive(Debug, Clone)]
pub struct PartyInfo {
    pub handle: String,
    pub display_name: String,
}

// Brand tokens (match web/src/index.css)
const PINK: (f32, f32, f32) = (1.0, 0.239, 0.498); // #ff3d7f
const MINT: (f32, f32, f32) = (0.239, 1.0, 0.710); // #3dffb5
const VOID: (f32, f32, f32) = (0.027, 0.027, 0.031); // #070708
const PANEL: (f32, f32, f32) = (0.071, 0.071, 0.078); // #121214
const SNOW: (f32, f32, f32) = (0.980, 0.980, 0.980); // #fafafa
const FOG: (f32, f32, f32) = (0.631, 0.631, 0.667); // #a1a1aa
const INK: (f32, f32, f32) = (0.08, 0.08, 0.09);

fn rgb(c: (f32, f32, f32)) -> Color {
    Color::Rgb(Rgb::new(c.0, c.1, c.2, None))
}

fn assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("BANK_ASSETS_DIR") {
        return PathBuf::from(p);
    }
    for cand in ["assets", "/app/assets", "./assets"] {
        let p = Path::new(cand);
        if p.join("fonts/DejaVuSans.ttf").exists() {
            return p.to_path_buf();
        }
    }
    PathBuf::from("assets")
}

fn read_font(name: &str) -> Result<Vec<u8>, String> {
    let path = assets_dir().join("fonts").join(name);
    fs::read(&path).map_err(|e| format!("шрифт {name}: {e} ({})", path.display()))
}

fn format_money(minor: i64, currency: &str) -> String {
    let sign = if minor < 0 { "−" } else { "" };
    let abs = minor.unsigned_abs();
    let whole = abs / 100;
    let frac = abs % 100;
    let whole_s = format!("{whole}");
    let mut grouped = String::new();
    for (i, ch) in whole_s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(ch);
    }
    let grouped: String = grouped.chars().rev().collect();
    let ccy = match currency {
        "RUB" => "₽",
        "USD" => "$",
        "EUR" => "€",
        other => other,
    };
    format!("{sign}{grouped},{frac:02} {ccy}")
}

fn format_dt(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&chrono::FixedOffset::east_opt(3 * 3600).unwrap())
        .format("%d.%m.%Y %H:%M:%S (МСК)")
        .to_string()
}

fn format_date(dt: DateTime<Utc>) -> String {
    dt.with_timezone(&chrono::FixedOffset::east_opt(3 * 3600).unwrap())
        .format("%d.%m.%Y")
        .to_string()
}

struct PdfCtx {
    doc: PdfDocumentReference,
    font: IndirectFontRef,
    bold: IndirectFontRef,
}

impl PdfCtx {
    fn new(title: &str) -> Result<(Self, PdfPageIndex, PdfLayerIndex), String> {
        let (doc, page, layer) = PdfDocument::new(title, Mm(210.0), Mm(297.0), "Layer");
        let regular = read_font("DejaVuSans.ttf")?;
        let bold_bytes = read_font("DejaVuSans-Bold.ttf")?;
        let font = doc
            .add_external_font(Cursor::new(regular))
            .map_err(|e| e.to_string())?;
        let bold = doc
            .add_external_font(Cursor::new(bold_bytes))
            .map_err(|e| e.to_string())?;
        Ok((Self { doc, font, bold }, page, layer))
    }

    fn layer(&self, page: PdfPageIndex, layer: PdfLayerIndex) -> PdfLayerReference {
        self.doc.get_page(page).get_layer(layer)
    }

    fn text(
        &self,
        layer: &PdfLayerReference,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
    ) {
        layer.use_text(
            s,
            size,
            Mm(x),
            Mm(y),
            if bold { &self.bold } else { &self.font },
        );
    }

    fn text_color(&self, layer: &PdfLayerReference, color: (f32, f32, f32)) {
        layer.set_fill_color(rgb(color));
    }

    fn finish(self) -> Result<Vec<u8>, String> {
        self.doc.save_to_bytes().map_err(|e| e.to_string())
    }
}

fn circle(cx: f32, cy: f32, r: f32, n: usize) -> Vec<(Point, bool)> {
    (0..n)
        .map(|i| {
            let a = (i as f32 / n as f32) * TAU;
            (
                Point::new(Mm(cx + r * a.cos()), Mm(cy + r * a.sin())),
                false,
            )
        })
        .collect()
}

fn fill_poly(layer: &PdfLayerReference, points: Vec<(Point, bool)>, color: (f32, f32, f32)) {
    layer.set_fill_color(rgb(color));
    layer.set_outline_color(rgb(color));
    layer.set_outline_thickness(0.0);
    layer.add_polygon(Polygon {
        rings: vec![points],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    });
}

fn fill_rect(layer: &PdfLayerReference, x0: f32, y0: f32, x1: f32, y1: f32, color: (f32, f32, f32)) {
    fill_poly(
        layer,
        vec![
            (Point::new(Mm(x0), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y0)), false),
            (Point::new(Mm(x1), Mm(y1)), false),
            (Point::new(Mm(x0), Mm(y1)), false),
        ],
        color,
    );
}

/// Twin-orbit brand mark (pink mass + mint satellite + void core).
fn draw_brand_mark(layer: &PdfLayerReference, cx: f32, cy: f32, scale: f32) {
    // Approximate SVG BrandMark viewBox 0..28 → mm scale.
    let s = scale / 28.0;
    let ox = cx - 14.0 * s;
    let oy = cy - 14.0 * s;
    let map = |x: f32, y: f32| (ox + x * s, oy + (28.0 - y) * s);

    let (px, py) = map(11.2, 14.5);
    fill_poly(layer, circle(px, py, 8.4 * s, 48), PINK);
    let (mx, my) = map(18.2, 14.5);
    fill_poly(layer, circle(mx, my, 5.6 * s, 40), MINT);
    fill_poly(layer, circle(mx, my, 2.0 * s, 24), VOID);
}

fn draw_letterhead(
    ctx: &PdfCtx,
    layer: &PdfLayerReference,
    title: &str,
    doc_no: &str,
    issued_at: DateTime<Utc>,
) {
    fill_rect(layer, 0.0, 268.0, 210.0, 297.0, VOID);
    fill_rect(layer, 0.0, 266.5, 210.0, 268.0, PINK);
    fill_rect(layer, 0.0, 265.2, 70.0, 266.5, MINT);

    draw_brand_mark(layer, 24.0, 282.0, 14.0);

    ctx.text_color(layer, SNOW);
    ctx.text(layer, "laefye", 34.0, 284.0, 16.0, true);
    ctx.text_color(layer, FOG);
    ctx.text(layer, "BANK", 72.0, 284.5, 9.0, true);
    ctx.text(layer, "passkey neo-bank · laefye.bank", 34.0, 276.0, 7.0, false);

    ctx.text_color(layer, INK);
    ctx.text(layer, title, 20.0, 255.0, 14.0, true);
    ctx.text_color(layer, FOG);
    ctx.text(layer, &format!("Документ № {doc_no}"), 20.0, 248.0, 9.0, false);
    ctx.text(
        layer,
        &format!("Сформирован: {}", format_dt(issued_at)),
        20.0,
        242.0,
        8.0,
        false,
    );
    ctx.text_color(layer, INK);
}

fn qr_image(payload: &str) -> Result<Image, String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    let modules = code.width();
    let quiet = 2usize;
    let scale = 5usize;
    let size = (modules + quiet * 2) * scale;
    let mut data = vec![255u8; size * size];
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] == qrcode::Color::Dark {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = (x + quiet) * scale + dx;
                        let py = (y + quiet) * scale + dy;
                        data[py * size + px] = 0;
                    }
                }
            }
        }
    }
    Ok(Image {
        image: ImageXObject {
            width: Px(size),
            height: Px(size),
            color_space: ColorSpace::Greyscale,
            bits_per_component: ColorBits::Bit8,
            interpolate: false,
            image_data: data,
            image_filter: None,
            clipping_bbox: None,
            smask: None,
        },
    })
}

fn draw_dsm_block(ctx: &PdfCtx, layer: &PdfLayerReference, seal: &DocumentSeal, y_top: f32) {
    fill_rect(layer, 16.0, 12.0, 194.0, y_top, PANEL);
    fill_rect(layer, 16.0, y_top - 1.2, 194.0, y_top, MINT);

    ctx.text_color(layer, SNOW);
    ctx.text(layer, "DSM / TSE-подобный фискальный блок", 20.0, y_top - 7.0, 8.0, true);
    ctx.text_color(layer, FOG);

    let sig_short = if seal.signature_b64.len() > 28 {
        format!("{}…", &seal.signature_b64[..28])
    } else {
        seal.signature_b64.clone()
    };
    let lines = [
        format!("TSE-Seriennr.: {}", seal.serial),
        format!("Trans.-Nr: {}", seal.transaction_number),
        format!("Sig.-Zähler: {}", seal.signature_counter),
        format!("Alg: {}", seal.sig_alg),
        format!("Zeit: {}", format_dt(seal.signed_at)),
        format!("Signatur: {sig_short}"),
    ];
    let mut y = y_top - 14.0;
    for line in lines {
        ctx.text(layer, &line, 20.0, y, 7.0, false);
        y -= 4.2;
    }
    ctx.text(
        layer,
        "Проверка: QR = DSFinV-K-подобный V0 payload · программный SE Laefye DSM",
        20.0,
        16.0,
        6.0,
        false,
    );

    if let Ok(img) = qr_image(&seal.qr_payload) {
        img.add_to_layer(
            layer.clone(),
            ImageTransform {
                translate_x: Some(Mm(148.0)),
                translate_y: Some(Mm(18.0)),
                rotate: None,
                scale_x: None,
                scale_y: None,
                dpi: Some(220.0),
            },
        );
    }
    ctx.text_color(layer, INK);
}

pub fn render_statement(
    party: &PartyInfo,
    account: &AccountView,
    rows: &[TxnView],
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    seal: &DocumentSeal,
) -> Result<Vec<u8>, String> {
    let (ctx, page, layer_idx) = PdfCtx::new("Выписка по счёту — Laefye Bank")?;
    let layer = ctx.layer(page, layer_idx);

    let now = Utc::now();
    let doc_no = format!(
        "ВЫП-{}-{}",
        now.format("%Y%m%d"),
        &account.id.to_string()[..8]
    );

    draw_letterhead(&ctx, &layer, "Выписка по счёту", &doc_no, now);

    let period = match (from, to) {
        (Some(a), Some(b)) => format!("{} — {}", format_date(a), format_date(b)),
        (Some(a), None) => format!("с {}", format_date(a)),
        (None, Some(b)) => format!("по {}", format_date(b)),
        _ => "весь период (последние операции)".into(),
    };

    let mut y = 232.0;
    for line in [
        format!("Клиент: {} (@{})", party.display_name, party.handle),
        format!("Счёт: {} · {}", account.id, account.currency),
        format!(
            "Остаток на момент формирования: {}",
            format_money(account.balance_minor, &account.currency)
        ),
        format!("Период: {period}"),
    ] {
        ctx.text(&layer, &line, 20.0, y, 9.0, false);
        y -= 6.0;
    }

    y -= 4.0;
    fill_rect(&layer, 18.0, y - 1.0, 192.0, y + 5.0, (0.95, 0.95, 0.96));
    ctx.text(&layer, "Дата", 20.0, y, 8.0, true);
    ctx.text(&layer, "Описание", 48.0, y, 8.0, true);
    ctx.text(&layer, "Сумма", 155.0, y, 8.0, true);
    y -= 8.0;

    if rows.is_empty() {
        ctx.text(&layer, "Операций за выбранный период нет.", 20.0, y, 9.0, false);
    } else {
        for t in rows.iter().take(36) {
            if y < 78.0 {
                ctx.text(
                    &layer,
                    "… список сокращён; сузьте период для полной выписки",
                    20.0,
                    y,
                    8.0,
                    false,
                );
                break;
            }
            let date = format_date(t.created_at);
            let memo = if t.memo.is_empty() {
                "Операция".to_string()
            } else {
                t.memo.chars().take(42).collect()
            };
            let amt = format_money(t.amount_minor, &t.currency);
            ctx.text(&layer, &date, 20.0, y, 8.0, false);
            ctx.text(&layer, &memo, 48.0, y, 8.0, false);
            ctx.text(&layer, &amt, 155.0, y, 8.0, false);
            y -= 5.5;
        }
    }

    draw_dsm_block(&ctx, &layer, seal, 70.0);
    ctx.finish()
}

pub fn render_receipt(
    party: &PartyInfo,
    detail: &TxnDetail,
    counterparty: Option<&PartyInfo>,
    seal: &DocumentSeal,
) -> Result<Vec<u8>, String> {
    let (ctx, page, layer_idx) = PdfCtx::new("Квитанция об операции — Laefye Bank")?;
    let layer = ctx.layer(page, layer_idx);

    let doc_no = format!("КВ-{}", &detail.entry_id.to_string()[..8].to_uppercase());
    let direction = if detail.amount_minor >= 0 {
        "Зачисление"
    } else {
        "Списание"
    };

    draw_letterhead(&ctx, &layer, "Квитанция об операции", &doc_no, Utc::now());

    let mut y = 230.0;
    let lines: Vec<(String, bool)> = vec![
        (format!("Дата и время: {}", format_dt(detail.created_at)), false),
        (format!("Тип: {direction}"), false),
        (
            format!(
                "Сумма: {}",
                format_money(detail.amount_minor, &detail.currency)
            ),
            true,
        ),
        (format!("Валюта: {}", detail.currency), false),
        (
            format!(
                "Назначение: {}",
                if detail.memo.is_empty() {
                    "—"
                } else {
                    &detail.memo
                }
            ),
            false,
        ),
        (
            format!("Клиент: {} (@{})", party.display_name, party.handle),
            false,
        ),
        (format!("Счёт: {}", detail.account_id), false),
        (
            match counterparty {
                Some(c) => format!("Контрагент: {} (@{})", c.display_name, c.handle),
                None => "Контрагент: —".into(),
            },
            false,
        ),
        (format!("ID операции: {}", detail.entry_id), false),
        (format!("Статус: {}", status_ru(&detail.status)), false),
        (
            format!("Ключ идемпотентности: {}", detail.idempotency_key),
            false,
        ),
    ];

    for (line, emphasize) in lines {
        if emphasize {
            ctx.text_color(&layer, PINK);
            ctx.text(&layer, &line, 20.0, y, 12.0, true);
            ctx.text_color(&layer, INK);
        } else {
            ctx.text(&layer, &line, 20.0, y, 10.0, false);
        }
        y -= 8.5;
    }

    y -= 4.0;
    ctx.text_color(&layer, FOG);
    ctx.text(
        &layer,
        "Квитанция подтверждает учёт операции в книге банка и защищена DSM-печатью.",
        20.0,
        y,
        8.0,
        false,
    );
    ctx.text_color(&layer, INK);

    draw_dsm_block(&ctx, &layer, seal, 78.0);
    ctx.finish()
}

fn status_ru(s: &str) -> &str {
    match s {
        "posted" => "проведена",
        "void" => "отменена",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_seal() -> DocumentSeal {
        DocumentSeal {
            serial: "LAEFYE-DSM-TESTSERIAL01".into(),
            process_type: "Kassenbeleg-V1".into(),
            process_data: "Beleg^1.0^Betrag:-10000^Waehrung:RUB^Vorgang:abc^Kunde:demo".into(),
            transaction_number: 42,
            signature_counter: 42,
            signed_at: Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap(),
            signature_b64: "dGVzdC1zaWduYXR1cmUtYmxvYg==".into(),
            previous_signature_b64: String::new(),
            public_key_b64: "cHVia2V5".into(),
            qr_payload: "V0;LAEFYE-DSM-TESTSERIAL01;Kassenbeleg-V1;Beleg^1.0;42;42;2026-08-09T12:00:00Z;2026-08-09T12:00:00Z;ed25519;unixTime;dGVzdC1zaWduYXR1cmUtYmxvYg==;cHVia2V5".into(),
            sig_alg: "ed25519",
        }
    }

    #[test]
    fn branded_receipt_pdf_has_header_bytes() {
        let party = PartyInfo {
            handle: "demo".into(),
            display_name: "Demo User".into(),
        };
        let detail = TxnDetail {
            entry_id: uuid::Uuid::nil(),
            account_id: uuid::Uuid::nil(),
            amount_minor: -12_500,
            currency: "RUB".into(),
            memo: "Тест перевода".into(),
            status: "posted".into(),
            idempotency_key: "idem-1".into(),
            created_at: Utc.with_ymd_and_hms(2026, 8, 9, 10, 0, 0).unwrap(),
            counterparty_user_id: None,
            counterparty_account_id: None,
            direction: "debit".into(),
        };
        let pdf = render_receipt(&party, &detail, None, &sample_seal()).expect("pdf");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 2000);
    }
}
