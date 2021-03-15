use std::error::Error;

#[tokio::test]
async fn test() -> Result<(), Box<dyn Error>> {
    jackdaw::Client::connect("localhost:9092").await?;
    Ok(())
}
