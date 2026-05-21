export type NoteKind = 'requirement' | 'project';
export type NoteStatus = 'draft' | 'confirmed' | 'waiting' | 'running';
export type AgentState = 'running' | 'blocked' | 'idle';

export interface WorkspaceSummary {
  id: string;
  label: string;
  title: string;
  meta: string;
  active?: boolean;
}

export interface AgentSummary {
  id: string;
  label: string;
  title: string;
  state: AgentState;
  meta: string;
}

export interface BoardNote {
  id: string;
  kind: NoteKind;
  tone: string;
  kicker: string;
  title: string;
  status: NoteStatus;
  statusLabel: string;
  agent?: string;
  agentLabel?: string;
  description: string;
  tasks: string[];
}
