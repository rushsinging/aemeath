const database = db.getSiblingDB("aemeath");

const collections = [
  "workspaces",
  "chats",
  "chat_messages",
  "requirements",
  "projects",
  "project_tasks",
  "project_task_results",
  "agent_instances",
  "agent_heartbeats",
  "idempotency_records",
  "board_index",
  "reflections"
];

for (const name of collections) {
  if (!database.getCollectionNames().includes(name)) {
    database.createCollection(name);
  }
}

database.workspaces.createIndex({ tenant_id: 1, created_at: -1 });
database.chats.createIndex({ workspace_id: 1, created_at: -1 });
database.chat_messages.createIndex({ workspace_id: 1, chat_id: 1, created_at: -1 });
database.requirements.createIndex({ workspace_id: 1, status: 1, updated_at: -1 });
database.projects.createIndex({ workspace_id: 1, status: 1, updated_at: -1 });
database.project_tasks.createIndex({ workspace_id: 1, project_id: 1, status: 1 });
database.agent_instances.createIndex({ workspace_id: 1, role: 1, status: 1 });
database.agent_heartbeats.createIndex({ agent_id: 1 }, { unique: true });
database.idempotency_records.createIndex({ created_at: 1 }, { expireAfterSeconds: 3600 });
database.idempotency_records.createIndex({ key: 1, entity_type: 1, scope: 1 }, { unique: true });
database.board_index.createIndex({ snapshot_id: 1 }, { unique: true });
