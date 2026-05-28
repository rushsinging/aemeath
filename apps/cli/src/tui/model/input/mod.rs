pub mod attachment;
pub mod change;
pub mod completion;
pub mod document;
pub mod history;
pub mod intent;
pub mod model;
pub mod submission;

pub use attachment::InputAttachment;
pub use change::InputChange;
pub use completion::InputCompletion;
pub use document::{InputDocument, InputSelection};
pub use history::InputHistory;
pub use intent::InputIntent;
pub use model::InputModel;
pub use submission::InputSubmission;
