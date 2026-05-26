// bin/migrate_posts.rs
use mongodb::{bson::{doc, oid::ObjectId}, options::ClientOptions, Client, Collection};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
struct Post {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub _id: Option<ObjectId>,
    pub user_id: String,
    pub user_name: String,
    pub caption: Option<String>,
    pub image_url: Option<String>,
    pub post_type: Option<String>,
    // Add other fields as needed
}

#[tokio::main]
async fn main() -> mongodb::error::Result<()> {
    println!("Starting post migration...");

    // Connect to MongoDB
    let client_options = ClientOptions::parse("mongodb://localhost:27017").await?;
    let client = Client::with_options(client_options)?;
    let db = client.database("fanclash");
    let collection: Collection<Post> = db.collection("posts");

    // Find all posts without post_type
    let filter = doc! { "post_type": { "$exists": false } };
    let cursor = collection.find(filter.clone()).await?;
    let posts: Vec<Post> = cursor.try_collect().await?;

    println!("Found {} posts without post_type", posts.len());

    let mut updated = 0;

    for mut post in posts {
        // Determine post type from content
        let post_type = match (post.caption.is_some(), post.image_url.is_some()) {
            (true, true) => "TextAndImage",
            (true, false) => "Text",
            (false, true) => "Image",
            (false, false) => "Text",
        };

        // Update the document
        let update = doc! {
            "$set": { "post_type": post_type }
        };

        if let Some(id) = post._id {
            let result = collection.update_one(
                doc! { "_id": id },
                update
            ).await?;

            if result.modified_count > 0 {
                updated += 1;
                println!("Updated post {} with type {}", id, post_type);
            }
        }
    }

    println!("Migration complete! Updated {} posts", updated);
    Ok(())
}
