use memorize_core::Store;
use memorize_proto::memorize_server::Memorize;
use memorize_proto::{
    Command, CommandResponse, ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse,
    ErrorResponse, GetRequest, GetResponse, KeysRequest, KeysResponse, SetRequest, SetResponse,
};
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status};

/// The gRPC service implementation
pub struct MemorizeService {
    store: Store,
}

impl MemorizeService {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[tonic::async_trait]
impl Memorize for MemorizeService {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("GET {}", key);

        let value = self.store.get(key);
        Ok(Response::new(GetResponse { value }))
    }

    async fn set(&self, request: Request<SetRequest>) -> Result<Response<SetResponse>, Status> {
        let req = request.get_ref();
        tracing::debug!("SET {} (ttl: {}s)", req.key, req.ttl_seconds);

        self.store.set(&req.key, &req.value, req.ttl_seconds);

        Ok(Response::new(SetResponse { success: true }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("DELETE {}", key);

        let deleted = self.store.delete(key);

        Ok(Response::new(DeleteResponse { deleted }))
    }

    async fn keys(&self, _request: Request<KeysRequest>) -> Result<Response<KeysResponse>, Status> {
        tracing::debug!("KEYS");

        let keys = self.store.keys();

        Ok(Response::new(KeysResponse { keys }))
    }

    async fn contains(
        &self,
        request: Request<ContainsRequest>,
    ) -> Result<Response<ContainsResponse>, Status> {
        let key = &request.get_ref().key;
        tracing::debug!("CONTAINS {}", key);

        let exists = self.store.contains_key(key);

        Ok(Response::new(ContainsResponse { exists }))
    }

    type ExecuteStream = Pin<Box<dyn Stream<Item = Result<CommandResponse, Status>> + Send>>;

    async fn execute(
        &self,
        request: Request<tonic::Streaming<Command>>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let store = self.store.clone();
        let mut stream = request.into_inner();

        let (tx, rx) = mpsc::channel(128);

        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                let response = match result {
                    Ok(cmd) => process_command(&store, cmd),
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        continue;
                    }
                };

                if tx.send(Ok(response)).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }
}

/// Process a single command from the streaming RPC
fn process_command(store: &Store, cmd: Command) -> CommandResponse {
    let id = cmd.id.clone();

    let response = match cmd.command {
        Some(memorize_proto::command::Command::Get(req)) => {
            let value = store.get(&req.key);
            memorize_proto::command_response::Response::Get(GetResponse { value })
        }
        Some(memorize_proto::command::Command::Set(req)) => {
            store.set(&req.key, &req.value, req.ttl_seconds);
            memorize_proto::command_response::Response::Set(SetResponse { success: true })
        }
        Some(memorize_proto::command::Command::Delete(req)) => {
            let deleted = store.delete(&req.key);
            memorize_proto::command_response::Response::Delete(DeleteResponse { deleted })
        }
        Some(memorize_proto::command::Command::Keys(_)) => {
            let keys = store.keys();
            memorize_proto::command_response::Response::Keys(KeysResponse { keys })
        }
        Some(memorize_proto::command::Command::Contains(req)) => {
            let exists = store.contains_key(&req.key);
            memorize_proto::command_response::Response::Contains(ContainsResponse { exists })
        }
        None => memorize_proto::command_response::Response::Error(ErrorResponse {
            message: "Empty command".to_string(),
            code: 400,
        }),
    };

    CommandResponse {
        id,
        response: Some(response),
    }
}
