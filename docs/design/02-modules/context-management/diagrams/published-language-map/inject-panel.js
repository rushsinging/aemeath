/* Context 根面 PL 设计图 — 符号级展开面板（archify 底图之上）。
 *
 * 数据由 ../symbol-data.json 内联注入（DATA 常量），内容来自：
 *   1) context/src/lib.rs 根导出清单
 *   2) syn AST 扫描（Call/Struct/Type 节点，跳过 use）→ 消费者与调用源码位置
 * 单击 W 组节点打开面板，单击符号行展开类型/字段/方法/消费者，单击消费者展开源码。
 */
(function () {
  const DATA = __PL_DATA__;
  const css = document.createElement('style');
  css.textContent = `
    .pl-panel{position:fixed;top:0;right:-560px;width:540px;height:100vh;background:#fff;
      box-shadow:-4px 0 24px rgba(0,0,0,.18);z-index:9999;transition:right .25s ease;
      font-family:ui-monospace,Menlo,monospace;overflow-y:auto;overflow-x:hidden;padding:0 0 40px}
    .pl-panel.open{right:0}
    .pl-head{position:sticky;top:0;background:#1e293b;color:#fff;padding:14px 18px;
      display:flex;justify-content:space-between;align-items:center;z-index:2}
    .pl-head h3{margin:0;font-size:15px;font-weight:600}
    .pl-close{cursor:pointer;font-size:20px;color:#94a3b8;background:none;border:none}
    .pl-close:hover{color:#fff}
    .pl-hint{background:#0f172a;color:#64748b;font-size:10px;padding:5px 18px}
    .pl-sym{border-bottom:1px solid #e2e8f0;padding:12px 18px}
    .pl-deep{font-size:9px;padding:0 5px;border-radius:2px;background:#ede9fe;color:#6d28d9;font-weight:600}
    .pl-chains{margin:5px 0;padding:5px 8px;background:#f0fdf4;border-left:3px solid #16a34a;border-radius:0 4px 4px 0;font-size:10.5px;line-height:1.6}
    .pl-chain-name{font-weight:700;color:#15803d}
    .pl-chain-path{margin:2px 0 4px 8px;padding-left:8px;border-left:2px solid #bbf7d0;color:#166534;font-size:10px;line-height:1.55}
    .pl-chain-step{margin:1px 0}
    .pl-gchain{margin:0 0 10px;padding:9px 11px;background:#f0fdf4;border:1px solid #bbf7d0;border-radius:5px}
    .pl-gchain-title{font-weight:700;color:#15803d;font-size:11.5px;margin-bottom:5px}
    .pl-gchain-step{color:#166534;font-size:10.5px;line-height:1.6;margin:1px 0}
    .pl-duty{color:#475569;font-size:11px;line-height:1.55;margin:4px 0 2px;
      overflow-wrap:anywhere;white-space:normal}
    .pl-sym-name{font-size:13px;font-weight:700;color:#0f172a;cursor:pointer;display:flex;
      align-items:center;gap:8px;flex-wrap:wrap;user-select:none}
    .pl-sym-name:hover{color:#2563eb}
    .pl-kind{font-size:10px;padding:1px 6px;border-radius:3px;font-weight:600}
    .k-struct{background:#dbeafe;color:#1d4ed8}.k-enum{background:#fef3c7;color:#b45309}
    .k-trait{background:#d1fae5;color:#047857}.k-fn{background:#fce7f3;color:#be185d}
    .k-module{background:#f1f5f9;color:#475569}.k-type{background:#ede9fe;color:#6d28d9}
    .pl-detail{display:none;margin-top:8px;font-size:11px;line-height:1.7;color:#334155;
      overflow-wrap:anywhere;word-break:break-all;white-space:normal;max-width:100%}
    .pl-sym.expanded .pl-detail{display:block}
    .pl-sec{color:#64748b;font-size:10px;letter-spacing:.1em;margin:6px 0 2px}
    .pl-field{color:#7c2d12;background:#fff7ed;padding:0 4px;border-radius:2px;margin:1px 0;
      overflow-wrap:anywhere;word-break:break-all;white-space:normal;overflow:hidden}
    .pl-method{color:#1e3a5f;background:#eff6ff;padding:0 4px;border-radius:2px;margin:1px 0;
      overflow-wrap:anywhere;word-break:break-all;white-space:normal;overflow:hidden}
    .pl-cons{cursor:pointer;transition:background .15s;margin:3px 0;padding:5px 8px;
      background:#f8fafc;border-left:2px solid #94a3b8;border-radius:0 3px 3px 0}
    .pl-cons:hover{background:#eef2f7}
    .pl-cons-head{display:flex;align-items:center;gap:6px;flex-wrap:wrap}
    .pl-cons-file{color:#0f766e;font-size:10px;overflow-wrap:anywhere;word-break:break-all}
    .pl-cons-toggle{color:#2563eb;font-size:10px;margin-left:auto;white-space:nowrap}
    .pl-src{display:none;margin-top:6px}
    .pl-cons.opened .pl-src{display:block}
    .pl-src-seg{background:#0f172a;border-radius:4px;padding:6px 8px;margin:4px 0;overflow-x:auto}
    .pl-src-line{color:#94a3b8;font-size:9px;margin-bottom:3px;display:flex;gap:8px;align-items:center}
    .pl-src-code{font-size:10px;line-height:1.55;white-space:pre;font-family:ui-monospace,Menlo,monospace;color:#94a3b8}
    .pl-src-code>span{display:block}
    .pl-src-code .hit{display:block;color:#fbbf24;font-weight:600;background:rgba(251,191,36,.10);
      border-left:2px solid #fbbf24;margin-left:-8px;padding-left:6px}
    .pl-kind-src{font-size:9px;padding:0 5px;border-radius:2px;font-weight:600}
    .pl-k-call{color:#047857;background:#d1fae5}
    .pl-k-ctor{color:#b45309;background:#fef3c7}
    .pl-k-implty{color:#6d28d9;background:#ede9fe}
    .pl-k-fnty{color:#475569;background:#f1f5f9}
    .pl-count{font-size:10px;color:#94a3b8}
    .pl-empty{padding:8px;color:#94a3b8;font-size:11px}
  `;
  document.head.appendChild(css);

  const panel = document.createElement('div');
  panel.className = 'pl-panel';
  panel.innerHTML = '<div class="pl-head"><h3 id="pl-title"></h3>' +
    '<span><span id="pl-count" class="pl-count" style="margin-right:14px"></span>' +
    '<button class="pl-close">✕</button></span></div>' +
    '<div class="pl-hint">每符号常显职责；点击展开类型 / 字段 / 方法 / 消费者，消费者可展开调用源码（▶ 命中行）· Esc 关闭</div>' +
    '<div id="pl-body"></div>';
  document.body.appendChild(panel);
  panel.querySelector('.pl-close').onclick = () => panel.classList.remove('open');
  document.addEventListener('keydown', e => { if (e.key === 'Escape') panel.classList.remove('open'); });

  const esc = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;');

  function render(groupName) {
    const syms = DATA.groups[groupName] || [];
    document.getElementById('pl-title').textContent = groupName;
    document.getElementById('pl-count').textContent = syms.length + ' 符号';
    const body = document.getElementById('pl-body');
    const gc = (DATA.groupChains || {})[groupName];
    const chainBlock = gc ? '<div class="pl-gchain"><div class="pl-gchain-title">完整链路 · ' + esc(gc.chain) + '</div>' +
      gc.path.map((st, i) => '<div class="pl-gchain-step">' + (i + 1) + '. ' + esc(st) + '</div>').join('') + '</div>' : '';
    body.innerHTML = chainBlock + syms.map(name => {
      const s = DATA.symbols[name];
      if (!s) return '';
      const fields = (s.fields || []).map(f => '<div class="pl-field">' + esc(f) + '</div>').join('');
      const methods = (s.methods || []).map(m => '<div class="pl-method">' + esc(m) + '</div>').join('');
      const consumers = (s.consumers || []).map(c => {
        const segs = (c.snippets || []).map(sn => {
          const rows = sn.code.split('\n');
          const at = Math.max(0, Math.min(rows.length - 1, sn.at || 0));
          const code = rows.map((r, idx) => idx === at
            ? '<span class="hit">▶ ' + esc(r) + '</span>'
            : '<span>  ' + esc(r) + '</span>').join('');
          return '<div class="pl-src-seg"><div class="pl-src-line"><span>L' + sn.line + '</span>' +
            '<span class="pl-kind-src pl-k-' + sn.kind + '">' + (sn.label || sn.kind) + '</span></div>' +
            '<div class="pl-src-code">' + code + '</div></div>';
        }).join('');
        const n = (c.snippets || []).length;
        return '<div class="pl-cons"><div class="pl-cons-head">' +
          '<div class="pl-cons-file">' + esc(c.module) + '</div>' +
          (n ? '<span class="pl-cons-toggle">▸ ' + n + ' 处</span>' : '') + '</div>' +
          (n ? '<div class="pl-src">' + segs + '</div>' : '') + '</div>';
      }).join('');
      return '<div class="pl-sym"><div class="pl-sym-name">' +
        '<span class="pl-kind k-' + (s.kind || 'module') + '">' + esc(s.kind) + '</span>' + esc(name) +
        (s.deep_path ? '<span class="pl-deep">' + esc(s.deep_path) + '::</span>' : '') +
        '<span style="margin-left:auto;color:#94a3b8;font-size:10px">' + s.consumer_count + ' 消费 ▸</span></div>' +
        (s.duty ? '<div class="pl-duty">' + esc(s.duty) + '</div>' : '') +
        ((s.chains && s.chains.length) ? '<div class="pl-chains">' + s.chains.map(function (c) {
            return '<div><span class="pl-chain-name">' + esc(c.chain) + '</span>' +
              (c.role ? ' · ' + esc(c.role) : '') +
              ((c.path && c.path.length) ? '<div class="pl-chain-path">' + c.path.map(function (st, i) {
                return '<div class="pl-chain-step">' + (i + 1) + '. ' + esc(st) + '</div>';
              }).join('') + '</div>' : '') + '</div>';
        }).join('') + '</div>' : '') +
        '<div class="pl-detail">' +
          (s.signature ? '<div class="pl-sec">定义签名</div><div>' + esc(s.signature) + '</div>' : '') +
          (s.def ? '<div class="pl-sec">定义位置</div><div style="color:#64748b">' + esc(s.def) + '</div>' : '') +
          (fields ? '<div class="pl-sec">字段</div>' + fields : '') +
          (methods ? '<div class="pl-sec">方法</div>' + methods : '') +
          '<div class="pl-sec">消费者（点开看调用源码）</div>' +
          (consumers || '<div class="pl-empty">无外部 crate 消费</div>') +
        '</div></div>';
    }).join('');
    body.querySelectorAll('.pl-sym-name').forEach(el => {
      el.onclick = () => el.parentElement.classList.toggle('expanded');
    });
    body.querySelectorAll('.pl-cons').forEach(el => {
      if (!el.querySelector('.pl-src')) return;
      el.onclick = e => {
        if (e.target.closest('.pl-src')) return;
        el.classList.toggle('opened');
        const t = el.querySelector('.pl-cons-toggle');
        const n = el.querySelectorAll('.pl-src-seg').length;
        if (t) t.textContent = el.classList.contains('opened') ? '▾ 收起' : '▸ ' + n + ' 处';
      };
    });
    panel.classList.add('open');
  }

  const bind = setInterval(() => {
    for (const gname of Object.values(DATA.nodeGroup)) {
      const el = document.querySelector('[aria-label^="Focus ' + gname + '"]');
      if (el && !el.dataset.plBound) {
        el.dataset.plBound = '1';
        el.style.cursor = 'pointer';
        el.addEventListener('click', () => render(gname));
        el.addEventListener('keydown', e => { if (e.key === 'Enter') render(gname); });
      }
    }
  }, 600);
  setTimeout(() => clearInterval(bind), 20000);
})();
