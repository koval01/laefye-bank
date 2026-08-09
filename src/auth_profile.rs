//! Bank profiles (handles) in the auth database.

use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BankProfile {
    pub user_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub email: String,
    pub phone: String,
    pub full_name: String,
    pub birth_date: Option<NaiveDate>,
}

const PROFILE_COLS: &str =
    "user_id, handle, display_name, email, phone, full_name, birth_date";

pub async fn get_by_user(pool: &PgPool, user_id: Uuid) -> Result<Option<BankProfile>, sqlx::Error> {
    sqlx::query_as::<_, BankProfile>(&format!(
        "SELECT {PROFILE_COLS} FROM bank_profiles WHERE user_id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_by_handle(pool: &PgPool, handle: &str) -> Result<Option<BankProfile>, sqlx::Error> {
    sqlx::query_as::<_, BankProfile>(&format!(
        "SELECT {PROFILE_COLS} FROM bank_profiles WHERE handle = $1"
    ))
    .bind(handle)
    .fetch_optional(pool)
    .await
}

pub fn normalize_handle(raw: &str) -> Result<String, String> {
    let h = raw.trim().trim_start_matches('@').to_ascii_lowercase();
    if !regex_lite::Regex::new(r"^[a-z][a-z0-9_]{2,31}$")
        .map_err(|e| e.to_string())?
        .is_match(&h)
    {
        return Err(
            "Ник: от 3 до 32 символов, начинается с буквы, дальше a–z, цифры или _".into(),
        );
    }
    Ok(h)
}

pub fn normalize_email(raw: &str) -> Result<String, String> {
    let e = raw.trim().to_ascii_lowercase();
    if e.is_empty() {
        return Ok(String::new());
    }
    if e.len() > 254 || !e.contains('@') || e.starts_with('@') || e.ends_with('@') {
        return Err("Укажите корректный email".into());
    }
    Ok(e)
}

pub fn normalize_phone(raw: &str) -> Result<String, String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    if digits.is_empty() {
        return Ok(String::new());
    }
    let compact: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    if compact.len() < 10 || compact.len() > 20 {
        return Err("Телефон: 10–20 символов, можно с +".into());
    }
    Ok(compact)
}

pub fn normalize_full_name(raw: &str) -> Result<String, String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Ok(String::new());
    }
    if name.len() < 3 || name.len() > 120 {
        return Err("ФИО: от 3 до 120 символов".into());
    }
    Ok(name)
}

pub async fn upsert_profile(
    pool: &PgPool,
    user_id: Uuid,
    handle: &str,
    display_name: &str,
) -> Result<BankProfile, sqlx::Error> {
    sqlx::query_as::<_, BankProfile>(
        r#"
        INSERT INTO bank_profiles (user_id, handle, display_name)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id) DO UPDATE
          SET handle = EXCLUDED.handle,
              display_name = EXCLUDED.display_name,
              updated_at = NOW()
        RETURNING user_id, handle, display_name, email, phone, full_name, birth_date
        "#,
    )
    .bind(user_id)
    .bind(handle)
    .bind(display_name)
    .fetch_one(pool)
    .await
}

pub async fn update_display_name(
    pool: &PgPool,
    user_id: Uuid,
    display_name: &str,
) -> Result<Option<BankProfile>, sqlx::Error> {
    let name = display_name.trim();
    if name.is_empty() || name.len() > 64 {
        return Ok(None);
    }
    sqlx::query_as::<_, BankProfile>(
        r#"
        UPDATE bank_profiles
           SET display_name = $2, updated_at = NOW()
         WHERE user_id = $1
        RETURNING user_id, handle, display_name, email, phone, full_name, birth_date
        "#,
    )
    .bind(user_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub struct ProfileContacts {
    pub display_name: Option<String>,
    pub email: String,
    pub phone: String,
    pub full_name: String,
    pub birth_date: Option<NaiveDate>,
}

pub async fn update_contacts(
    pool: &PgPool,
    user_id: Uuid,
    contacts: &ProfileContacts,
) -> Result<Option<BankProfile>, sqlx::Error> {
    if let Some(ref name) = contacts.display_name {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return Ok(None);
        }
        sqlx::query_as::<_, BankProfile>(
            r#"
            UPDATE bank_profiles
               SET display_name = $2,
                   email = $3,
                   phone = $4,
                   full_name = $5,
                   birth_date = $6,
                   updated_at = NOW()
             WHERE user_id = $1
            RETURNING user_id, handle, display_name, email, phone, full_name, birth_date
            "#,
        )
        .bind(user_id)
        .bind(name)
        .bind(&contacts.email)
        .bind(&contacts.phone)
        .bind(&contacts.full_name)
        .bind(contacts.birth_date)
        .fetch_optional(pool)
        .await
    } else {
        sqlx::query_as::<_, BankProfile>(
            r#"
            UPDATE bank_profiles
               SET email = $2,
                   phone = $3,
                   full_name = $4,
                   birth_date = $5,
                   updated_at = NOW()
             WHERE user_id = $1
            RETURNING user_id, handle, display_name, email, phone, full_name, birth_date
            "#,
        )
        .bind(user_id)
        .bind(&contacts.email)
        .bind(&contacts.phone)
        .bind(&contacts.full_name)
        .bind(contacts.birth_date)
        .fetch_optional(pool)
        .await
    }
}

pub fn profile_complete(p: &BankProfile) -> bool {
    !p.email.is_empty()
        && !p.phone.is_empty()
        && !p.full_name.is_empty()
        && p.birth_date.is_some()
}
