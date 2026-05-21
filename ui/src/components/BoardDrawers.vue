<script setup lang="ts">
import type { AgentSummary, WorkspaceSummary } from '../types';

const props = defineProps<{
  workspaces: WorkspaceSummary[];
  agents: AgentSummary[];
}>();
</script>

<template>
  <aside class="drawer left" aria-label="workspace drawer">
    <div class="drawer-title"><span>Workspaces</span><i class="live-dot"></i></div>
    <section v-for="workspace in props.workspaces" :key="workspace.id" class="workspace-card" :class="{ active: workspace.active }">
      <div class="card-kicker">{{ workspace.label }}</div>
      <div class="card-title">{{ workspace.title }}</div>
      <div class="card-meta">{{ workspace.meta }}</div>
    </section>
    <section class="info-card">
      <div class="card-kicker">board snapshot</div>
      <div class="card-title">Whiteboard mode</div>
      <div class="card-meta">Click any sticky note to inspect details. Chat stays docked at the bottom.</div>
    </section>
  </aside>

  <aside class="drawer right" aria-label="agent drawer">
    <div class="drawer-title"><span>Agents</span><span class="status running"><i></i> live</span></div>
    <section v-for="agent in props.agents" :key="agent.id" class="agent-card">
      <div class="card-kicker">{{ agent.label }}</div>
      <div class="card-title">{{ agent.title }}</div>
      <div class="agent-row"><span class="status" :class="agent.state"><i></i> {{ agent.state }}</span><span class="card-meta">{{ agent.meta }}</span></div>
    </section>
    <section class="info-card">
      <div class="card-kicker">active focus</div>
      <div class="card-title">Board is primary</div>
      <div class="card-meta">Side drawers hold context so the canvas can stay uninterrupted.</div>
    </section>
  </aside>
</template>
