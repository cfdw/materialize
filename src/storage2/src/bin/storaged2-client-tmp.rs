use std::iter;

use futures::stream;

use mz_storage2::service::storage_client::StorageClient;
use mz_storage2::service::StorageCommand;

#[tokio::main]
async fn main() {
    let mut client = StorageClient::connect("http://localhost:2100")
        .await
        .unwrap();
    client
        .control(stream::iter(iter::once(StorageCommand {
            name: "hi".into(),
        })))
        .await
        .unwrap();
}
