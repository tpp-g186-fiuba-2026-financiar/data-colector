use dotenv::Error;
use std::env::VarError;

#[derive(Debug)]
pub enum DataCollectorError<'a> {
    EnviromentFileError(Error),
    EnviromentVariableError(&'a str, VarError),
    PosgresConnectionError(sqlx::Error),
    PostgresQueryError(sqlx::Error),
    TcpBindError(std::io::Error),
    AxumServeError(std::io::Error),
    BymaScrapperError(&'a str),
    BymaInformationNotAvailable(&'a str),
    PersistenceError(sqlx::Error),
    MigrationError(sqlx::migrate::MigrateError),
    YFinanceClientError(yfinance_rs::YfError),
    RavaScrapperError(&'a str),
}

impl std::fmt::Display for DataCollectorError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = "[DataCollectorError]";
        match self {
            DataCollectorError::EnviromentFileError(msg) => write!(f, "{}: {}", prefix, msg),
            DataCollectorError::EnviromentVariableError(var, err) => match err {
                VarError::NotPresent => {
                    write!(f, "{}: Environment variable '{}' is not set", prefix, var)
                }
                VarError::NotUnicode(os_str) => write!(
                    f,
                    "{}: Environment variable '{}' contains invalid unicode: {:?}",
                    prefix, var, os_str
                ),
            },
            DataCollectorError::PosgresConnectionError(err) => match err {
                sqlx::Error::Io(io_err) => write!(
                    f,
                    "{}: I/O error while connecting to Postgres: {}",
                    prefix, io_err
                ),
                sqlx::Error::Tls(tls_err) => write!(
                    f,
                    "{}: TLS error while connecting to Postgres: {}",
                    prefix, tls_err
                ),
                sqlx::Error::Protocol(protocol_err) => write!(
                    f,
                    "{}: Protocol error while connecting to Postgres: {}",
                    prefix, protocol_err
                ),
                sqlx::Error::Database(db_err) => write!(
                    f,
                    "{}: Database error while connecting to Postgres: {}",
                    prefix, db_err
                ),
                _ => write!(
                    f,
                    "{}: Unknown error while connecting to Postgres: {}",
                    prefix, err
                ),
            },
            DataCollectorError::TcpBindError(err) => {
                write!(f, "{}: Failed to bind to TCP address: {}", prefix, err)
            }
            DataCollectorError::AxumServeError(err) => {
                write!(f, "{}: Failed to start Axum server: {}", prefix, err)
            }
            DataCollectorError::BymaScrapperError(err) => {
                write!(f, "{}: Failed to initialize BymaScrapper: {}", prefix, err)
            }
            DataCollectorError::BymaInformationNotAvailable(info) => {
                write!(
                    f,
                    "{}: Information '{}' is not available from Byma",
                    prefix, info
                )
            }
            DataCollectorError::PersistenceError(err) => {
                write!(f, "{}: Persistence error: {}", prefix, err)
            }
            DataCollectorError::MigrationError(err) => {
                write!(f, "{}: Migration error: {}", prefix, err)
            }
            DataCollectorError::YFinanceClientError(err) => {
                write!(f, "{}: YFinance client error: {}", prefix, err)
            }
            DataCollectorError::RavaScrapperError(err) => {
                write!(f, "{}: Rava scrapper error: {}", prefix, err)
            }
            DataCollectorError::PostgresQueryError(err) => {
                write!(f, "{}: Postgres query error: {}", prefix, err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use std::ffi::OsString;

    #[test]
    fn formats_environment_file_and_invalid_unicode_variable_errors() {
        let file_error =
            DataCollectorError::EnviromentFileError(dotenv::Error::LineParse("bad=line".into(), 3));
        assert!(file_error.to_string().contains("[DataCollectorError]"));

        let invalid_unicode = DataCollectorError::EnviromentVariableError(
            "LANG",
            VarError::NotUnicode(OsString::from("\u{0}bad")),
        );
        assert!(
            invalid_unicode
                .to_string()
                .contains("contains invalid unicode")
        );
    }

    #[test]
    fn formats_tls_protocol_migration_and_yfinance_errors() {
        let tls_err = DataCollectorError::PosgresConnectionError(sqlx::Error::Tls(Box::new(
            std::io::Error::other("handshake failed"),
        )));
        assert!(tls_err.to_string().contains("TLS error"));

        let protocol_err = DataCollectorError::PosgresConnectionError(sqlx::Error::Protocol(
            "unexpected byte".to_string(),
        ));
        assert!(protocol_err.to_string().contains("Protocol error"));

        let migration_err =
            DataCollectorError::MigrationError(sqlx::migrate::MigrateError::VersionMissing(1));
        assert!(migration_err.to_string().contains("Migration error"));

        let yfinance_err =
            DataCollectorError::YFinanceClientError(yfinance_rs::YfError::NotFound {
                url: "https://example.invalid/missing".to_string(),
            });
        assert!(yfinance_err.to_string().contains("YFinance client error"));
    }

    #[sqlx::test]
    async fn formats_real_database_error_from_postgres(pool: PgPool) {
        let db_error = sqlx::query("SELECT * FROM this_table_does_not_exist")
            .execute(&pool)
            .await
            .unwrap_err();

        let wrapped = DataCollectorError::PosgresConnectionError(db_error);
        assert!(wrapped.to_string().contains("Database error"));
    }

    #[test]
    fn formats_configuration_io_domain_and_database_errors() {
        let missing =
            DataCollectorError::EnviromentVariableError("DATABASE_URL", VarError::NotPresent);
        assert!(missing.to_string().contains("DATABASE_URL"));

        let io = || std::io::Error::other("boom");
        assert!(
            DataCollectorError::TcpBindError(io())
                .to_string()
                .contains("Failed to bind")
        );
        assert!(
            DataCollectorError::AxumServeError(io())
                .to_string()
                .contains("Failed to start")
        );
        assert!(
            DataCollectorError::BymaScrapperError("offline")
                .to_string()
                .contains("offline")
        );
        assert!(
            DataCollectorError::BymaInformationNotAvailable("quotes")
                .to_string()
                .contains("quotes")
        );
        assert!(
            DataCollectorError::RavaScrapperError("invalid")
                .to_string()
                .contains("invalid")
        );

        assert!(
            DataCollectorError::PosgresConnectionError(sqlx::Error::Io(io()))
                .to_string()
                .contains("I/O error")
        );
        assert!(
            DataCollectorError::PosgresConnectionError(sqlx::Error::RowNotFound)
                .to_string()
                .contains("Unknown error")
        );
        assert!(
            DataCollectorError::PersistenceError(sqlx::Error::RowNotFound)
                .to_string()
                .contains("Persistence error")
        );
        assert!(
            DataCollectorError::PostgresQueryError(sqlx::Error::RowNotFound)
                .to_string()
                .contains("Postgres query error")
        );
    }
}
