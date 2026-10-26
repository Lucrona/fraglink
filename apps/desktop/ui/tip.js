/* Shared tooltip placement for every window. x/below/above are viewport (mouse) pixels.
   Text size scales the body with CSS zoom, so a fixed tip's style pixels are not viewport
   pixels: measure the real scale and clamp/flip in viewport pixels, on every edge. */
(function () {
  function place(el, x, below, above) {
    el.style.maxWidth = ''; el.style.left = '0px'; el.style.top = '0px';
    const r0 = el.getBoundingClientRect();
    const k = el.offsetWidth ? r0.width / el.offsetWidth : 1;
    const W = innerWidth, H = innerHeight, m = 2;
    if (r0.width > W - 2 * m) el.style.maxWidth = Math.floor((W - 2 * m) / k) + 'px';
    const r = el.getBoundingClientRect(), tw = r.width, th = r.height;
    let left = x + tw <= W - m ? x : x - tw - 8; // flip left of the pointer when no room
    left = Math.max(m, Math.min(left, W - tw - m));
    let top = below + th <= H - m ? below : above - th; // flip above when no room below
    top = Math.max(m, Math.min(top, H - th - m));
    el.style.left = (left / k) + 'px'; el.style.top = (top / k) + 'px';
  }
  window.flPlaceTip = place;
  /* Pages without their own tip handler (Options): turn native titles into the same tip. */
  window.flInstallTips = function (tip) {
    let timer = null, cur = null;
    const hide = () => { clearTimeout(timer); cur = null; tip.hidden = true; };
    document.addEventListener('mouseover', e => {
      const el = e.target.closest ? e.target.closest('[data-tip],[title]') : null;
      if (el && el.hasAttribute('title')) { if (!el.dataset.tip) el.dataset.tip = el.getAttribute('title'); el.removeAttribute('title'); }
      if (el === cur) return;
      hide(); if (!el || !el.dataset.tip) return;
      cur = el; const mx = e.clientX, my = e.clientY;
      timer = setTimeout(() => {
        if (!el.isConnected || cur !== el) return;
        tip.textContent = el.dataset.tip; tip.hidden = false;
        place(tip, mx + 4, my + 20, my - 6);
      }, 450);
    }, true);
    document.addEventListener('mousedown', hide, true);
    document.addEventListener('wheel', hide, { passive: true, capture: true });
    addEventListener('blur', hide);
  };
})();
