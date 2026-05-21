<template>
  <aside class="chat-dock" aria-label="floating chat dock">
    <div class="chat-preview">
      <div class="avatar">AI</div>
      <div>
        <div class="chat-title">Ask the board</div>
        <div class="chat-hint">底部悬浮。Hover 或聚焦时展开，不抢占白板。</div>
      </div>
      <div class="expand-note">focus to expand</div>
    </div>
    <div class="chat-expanded">
      <div class="message-row">
        <textarea class="message-input" placeholder="Ask an agent to split this requirement, inspect a project, or summarize blockers…"></textarea>
        <button class="send" aria-label="send message">↵</button>
      </div>
      <div class="quick-actions">
        <span>summarize blockers</span>
        <span>create project from REQ-06</span>
        <span>assign UI Builder Agent</span>
        <span>show latest watch delta</span>
      </div>
    </div>
  </aside>
</template>
