use mongodb::{Client, Database};
use std::env;

pub async fn get_db_client() -> (Client, Database) {
    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set as an environment variable");

    let client = Client::with_uri_str(&database_url)
        .await
        .expect("Failed to connect to MongoDB");

    let db_name = "clashdb"; // Change this to your actual database name
    let db = client.database(&db_name);

    // Verify database exists by listing collections
    match db.list_collection_names().await {
        Ok(collections) => {
            println!("✅ Connected to database: {}", db_name);
            println!("📂 Collections found: {:?}", collections);

            // Check if 'games' collection exists
            if !collections.contains(&"games".to_string()) {
                println!("⚠️ Warning: 'games' collection not found in database");
            }
        }
        Err(e) => {
            eprintln!(
                "❌ Database '{}' may not exist or is inaccessible: {}",
                db_name, e
            );
            // You could panic here or return an error
            // panic!("Database '{}' not found: {}", db_name, e);
        }
    }

    (client, db)
}
