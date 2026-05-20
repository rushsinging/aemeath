use crate::config::ServerConfig;
use mongodb::{Client, Database, options::ClientOptions};

pub async fn connect(config: &ServerConfig) -> mongodb::error::Result<Database> {
    let options = ClientOptions::parse(&config.mongo_uri).await?;
    let client = Client::with_options(options)?;
    Ok(client.database(&config.mongo_database))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_connect_function_is_exposed_for_server_startup() {
        assert_eq!(
            std::any::type_name_of_val(&super::connect),
            "server::db::connect"
        );
    }
}
