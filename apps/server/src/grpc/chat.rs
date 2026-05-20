use crate::model::app::{analyze_message_type, AppState, StoreError};
use crate::proto::aemeath::v1::chat_service_server::ChatService;
use crate::proto::aemeath::v1::{
    AddMessageRequest, AddMessageResponse, AnalyzeMessageRequest, AnalyzeMessageResponse, Chat,
    ChatEvent, CreateChatRequest, CreateChatResponse, DeleteChatRequest, Empty, GetChatRequest,
    GetChatResponse, ListChatsRequest, ListChatsResponse, UpdateChatRequest, UpdateChatResponse,
    WatchRequest,
};
use std::pin::Pin;
use tokio_stream::{Stream, empty};
use tonic::{Request, Response, Status};

#[derive(Debug, Clone)]
pub struct ChatGrpc {
    state: AppState,
}

impl ChatGrpc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ChatService for ChatGrpc {
    async fn create_chat(
        &self,
        request: Request<CreateChatRequest>,
    ) -> Result<Response<CreateChatResponse>, Status> {
        let request = request.into_inner();
        let chat = self
            .state
            .create_chat(&request.workspace_id, request.title)
            .map_err(status_from_store_error)?;
        Ok(Response::new(CreateChatResponse {
            chat: Some(chat.into()),
        }))
    }

    async fn add_message(
        &self,
        request: Request<AddMessageRequest>,
    ) -> Result<Response<AddMessageResponse>, Status> {
        let request = request.into_inner();
        let result = self
            .state
            .add_message(
                &request.workspace_id,
                &request.chat_id,
                request.role,
                request.content,
                request.idempotency_key,
            )
            .map_err(status_from_store_error)?;
        Ok(Response::new(AddMessageResponse {
            message: Some(result.message.into()),
            deduplicated: result.deduplicated,
        }))
    }

    async fn get_chat(
        &self,
        request: Request<GetChatRequest>,
    ) -> Result<Response<GetChatResponse>, Status> {
        let request = request.into_inner();
        let chat = self
            .state
            .get_chat(&request.workspace_id, &request.chat_id)
            .map_err(status_from_store_error)?;
        Ok(Response::new(GetChatResponse {
            chat: Some(chat.into()),
        }))
    }

    async fn list_chats(
        &self,
        request: Request<ListChatsRequest>,
    ) -> Result<Response<ListChatsResponse>, Status> {
        let request = request.into_inner();
        let chats = self
            .state
            .list_chats(&request.workspace_id)
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(Response::new(ListChatsResponse {
            chats,
            next_page_token: String::new(),
        }))
    }

    async fn update_chat(
        &self,
        request: Request<UpdateChatRequest>,
    ) -> Result<Response<UpdateChatResponse>, Status> {
        let request = request.into_inner();
        let chat = self
            .state
            .update_chat(
                &request.workspace_id,
                &request.chat_id,
                request.title,
                request.status,
            )
            .map_err(status_from_store_error)?;
        Ok(Response::new(UpdateChatResponse {
            chat: Some(chat.into()),
        }))
    }

    async fn delete_chat(
        &self,
        request: Request<DeleteChatRequest>,
    ) -> Result<Response<Empty>, Status> {
        let request = request.into_inner();
        self.state
            .delete_chat(&request.workspace_id, &request.chat_id)
            .map_err(status_from_store_error)?;
        Ok(Response::new(Empty {}))
    }

    async fn analyze_message(
        &self,
        request: Request<AnalyzeMessageRequest>,
    ) -> Result<Response<AnalyzeMessageResponse>, Status> {
        let request = request.into_inner();
        self.state
            .get_chat(&request.workspace_id, &request.chat_id)
            .map_err(status_from_store_error)?;
        let analysis = analyze_message_type(&request.content);
        Ok(Response::new(AnalyzeMessageResponse {
            message_type: analysis.message_type,
            reason: analysis.reason,
        }))
    }

    type WatchStream = Pin<Box<dyn Stream<Item = Result<ChatEvent, Status>> + Send + 'static>>;

    async fn watch(
        &self,
        _request: Request<WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        Ok(Response::new(Box::pin(empty()) as Self::WatchStream))
    }
}

impl From<crate::model::app::Chat> for Chat {
    fn from(chat: crate::model::app::Chat) -> Self {
        Self {
            id: chat.id,
            workspace_id: chat.workspace_id,
            title: chat.title,
            status: chat.status,
            created_at: chat.created_at.to_string(),
            updated_at: chat.updated_at.to_string(),
            version: chat.version,
        }
    }
}

impl From<crate::model::app::ChatMessage> for crate::proto::aemeath::v1::ChatMessage {
    fn from(message: crate::model::app::ChatMessage) -> Self {
        Self {
            id: message.id,
            chat_id: message.chat_id,
            workspace_id: message.workspace_id,
            sender_type: message.sender_type,
            role: message.role,
            content: message.content,
            idempotency_key: message.idempotency_key,
            created_at: message.created_at.to_string(),
            updated_at: message.updated_at.to_string(),
            version: message.version,
        }
    }
}

fn status_from_store_error(error: StoreError) -> Status {
    match error {
        StoreError::InvalidInput { field } => Status::invalid_argument(format!("字段 {field} 不能为空")),
        StoreError::NotFound { entity } => Status::not_found(format!("{entity} 不存在")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chat_grpc_create_chat_requires_workspace() {
        let service = ChatGrpc::new(AppState::default());

        let result = service
            .create_chat(Request::new(CreateChatRequest {
                workspace_id: "missing".to_string(),
                title: "General".to_string(),
            }))
            .await;

        assert!(matches!(result, Err(status) if status.code() == tonic::Code::NotFound));
    }
}
