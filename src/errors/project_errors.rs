use dotenv::Error;
use std::env::VarError;

#[derive(Debug)]
pub enum DataCollectorError<'a> {
    EnviromentFileError(Error),
    EnviromentVariableError(&'a str, VarError),
    PosgresConnectionError(sqlx::Error),
    TcpBindError(std::io::Error),
    AxumServeError(std::io::Error),
    BymaScrapperError(&'a str),
    BymaInformationNotAvailable(&'a str),
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
        }
    }
}
