/* Context Receipt 全链路图 — 节点详情面板（archify 底图之上）。
 * payload = flow-data.json 每项合入 spec 的 label（构建期由 _label 字段注入）。
 * 点击节点 → 卡片：干什么 / 输入→输出 / 缺陷（A/C/B 修复阶段）/ 代码引用。 */
(function () {
  'use strict';
  var DATA = __FLOW_DATA__;

  var style = document.createElement('style');
  style.textContent = [
    '.rf-card{position:fixed;right:18px;top:18px;width:380px;max-height:86vh;overflow:auto;',
    'background:#fffdf9;border:1px solid #d8d2c4;border-radius:6px;padding:14px 16px;',
    'box-shadow:0 8px 28px rgba(20,16,10,.18);z-index:60;font:12.5px/1.65 ui-monospace,monospace;color:#1c1917;display:none}',
    '.rf-close{float:right;cursor:pointer;color:#78716c;font-weight:700;padding:0 4px}',
    '.rf-title{font-weight:700;font-size:13.5px;margin:2px 0 6px}',
    '.rf-stage{display:inline-block;font-size:10px;padding:0 6px;border-radius:2px;background:#ede9fe;color:#6d28d9;font-weight:700;margin-left:6px;vertical-align:1px}',
    '.rf-duty{color:#44403c;margin:6px 0}',
    '.rf-io{background:#f5f5f4;border-radius:4px;padding:6px 8px;margin:6px 0;color:#57534e}',
    '.rf-io b{color:#1c1917}',
    '.rf-defect{border-left:3px solid #dc2626;background:#fef2f2;padding:6px 9px;margin:6px 0;border-radius:0 4px 4px 0}',
    '.rf-defect .rf-code{display:inline-block;background:#dc2626;color:#fff;font-weight:700;font-size:10px;border-radius:2px;padding:0 5px;margin-right:6px}',
    '.rf-ref{color:#78716c;font-size:11px;margin:3px 0 0;word-break:break-all}',
    '.rf-hint{position:fixed;left:18px;bottom:14px;font:11px ui-monospace,monospace;color:#a8a29e;z-index:60}'
  ].join('\n');
  document.head.appendChild(style);

  var card = document.createElement('div');
  card.className = 'rf-card';
  document.body.appendChild(card);

  var hint = document.createElement('div');
  hint.className = 'rf-hint';
  hint.textContent = '点击节点查看：干什么 · 输入→输出 · 缺陷(A/C/B) · 代码引用';
  document.body.appendChild(hint);

  function esc(s) { return String(s).replace(/[&<>"]/g, function (c) {
    return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); }

  // label → 详情 的索引（构建期每项已带 _label）
  var byLabel = {};
  Object.keys(DATA).forEach(function (id) {
    var lbl = DATA[id]._label;
    if (lbl) byLabel[lbl] = DATA[id];
  });

  function findDetail(target) {
    for (var el = target; el && el !== document; el = el.parentElement) {
      var aria = el.getAttribute && el.getAttribute('aria-label');
      if (!aria) continue;
      var keys = Object.keys(byLabel);
      for (var i = 0; i < keys.length; i++) {
        if (aria.indexOf(keys[i]) >= 0) return byLabel[keys[i]];
      }
    }
    return null;
  }

  function render(d) {
    var defects = (d.defects || []).map(function (x) {
      return '<div class="rf-defect"><span class="rf-code">' + esc(x.code) + '</span>' + esc(x.text) + '</div>';
    }).join('');
    var refs = (d.refs || []).map(function (r) { return '<div class="rf-ref">↳ ' + esc(r) + '</div>'; }).join('');
    card.innerHTML =
      '<span class="rf-close" id="rf-x">✕</span>' +
      '<div class="rf-title">' + esc(d._label) + '<span class="rf-stage">' + esc(d.stage) + '</span></div>' +
      '<div class="rf-duty">' + esc(d.duty) + '</div>' +
      '<div class="rf-io"><b>输入</b>：' + esc(d.io.in) + '<br><b>输出</b>：' + esc(d.io.out) + '</div>' +
      (defects || '<div class="rf-ref">（本节点无已知缺陷）</div>') +
      refs;
    card.style.display = 'block';
    card.querySelector('#rf-x').onclick = function () { card.style.display = 'none'; };
  }

  document.addEventListener('click', function (ev) {
    var d = findDetail(ev.target);
    if (d) render(d);
  }, true);
})();
