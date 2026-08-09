//! Double-entry ledger (EUR/USD/RUB minor units).
//! **Core money logic is PostgreSQL** — see `migrations/ledger/003_ledger_engine.sql`.

mod transfer;

pub use transfer::{
    convert_currency, ensure_checking, ensure_checking_with_welcome, get_entry_for_user,
    list_accounts_for_user, list_transactions, list_transactions_range, lookup_account,
    transfer_p2p, AccountView, FxConvertResult, LedgerError, TransferRequest, TransferResult,
    TxnDetail, TxnView,
};

use sqlx::PgPool;
use uuid::Uuid;

pub const SYSTEM_FLOAT_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001);
pub const CURRENCY_RUB: &str = "RUB";
pub const CURRENCY_EUR: &str = "EUR";

pub async fn connect(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPool::connect(url).await
}
