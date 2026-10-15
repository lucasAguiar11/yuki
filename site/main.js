// Live request: the hero form really sends GET releases/latest, like Yuki would, and wakes the cat.
(() => {
  const form = document.querySelector('[data-live]');
  if (!form) return;
  const hero = form.closest('.hero');
  const status = form.querySelector('[data-status]');
  const body = form.querySelector('[data-body]');
  const button = form.querySelector('button');
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const str = (v) => `<span class="s">"${esc(v)}"</span>`;
  const kv = (k, v, last) => `  <span class="k">"${k}"</span><span class="p">:</span> ${v}${last ? '' : '<span class="p">,</span>'}`;
  const size = (n) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`);

  async function send() {
    if (button.disabled) return;
    button.disabled = true;
    status.textContent = 'sending…';
    const t0 = performance.now();
    try {
      const res = await fetch(form.action, { headers: { Accept: 'application/vnd.github+json' } });
      const text = await res.text();
      const ms = Math.round(performance.now() - t0);
      const clock = new Date().toLocaleTimeString([], { hour12: false });
      status.innerHTML = `<span class="${res.ok ? 'ok' : 'err'}">${res.status} ${esc(res.statusText || (res.ok ? 'OK' : ''))}</span>  ${ms} ms  ${size(new Blob([text]).size)}  ${clock}`;
      if (!res.ok) throw new Error(JSON.parse(text).message || `HTTP ${res.status}`);
      const r = JSON.parse(text);
      const dmgs = r.assets.filter((a) => a.name.endsWith('.dmg'));
      body.innerHTML = '<pre>' + [
        '<span class="p">{</span>',
        kv('tag_name', str(r.tag_name)),
        kv('published_at', str(r.published_at.slice(0, 10))),
        kv('assets', '<span class="p">[</span>', true),
        ...dmgs.map((a, i) => `    <a href="${esc(a.browser_download_url)}">${str(a.name)}</a>${i < dmgs.length - 1 ? '<span class="p">,</span>' : ''}`),
        '  <span class="p">]</span>',
        '<span class="p">}</span>',
      ].join('\n') + '</pre>';
      hero.setAttribute('data-awake', '');
      document.querySelectorAll('[data-download]').forEach((a) => { a.textContent = `Download ${r.tag_name}`; });
    } catch (err) {
      if (!status.querySelector('.err')) status.innerHTML = '<span class="err">× send failed</span>';
      body.innerHTML = `<span class="err">${esc(err.message)}</span>\n<a href="https://github.com/lucasAguiar11/yuki/releases/latest">open the releases page</a>`;
    } finally {
      button.disabled = false;
    }
  }

  form.addEventListener('submit', (e) => { e.preventDefault(); send(); });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); send(); }
  });
})();

// Showcase index: pointing at (or tapping, or focusing) a name brings up its screen and recolours the head.
(() => {
  const show = document.querySelector('.show');
  if (!show) return;
  const disc = show.querySelector('.show__disc');
  const fact = show.querySelector('[data-fact]');
  const facts = Object.fromEntries([...show.querySelector('[data-facts]').content.children].map((p) => [p.dataset.for, p.textContent]));
  const pick = (btn) => {
    const id = btn.dataset.pick;
    show.querySelectorAll('[data-pick]').forEach((b) => b.setAttribute('aria-pressed', String(b === btn)));
    show.querySelectorAll('[data-shot]').forEach((img) => img.classList.toggle('is-on', img.dataset.shot === id));
    disc.style.fill = btn.dataset.disc;
    fact.textContent = facts[id];
  };
  show.querySelectorAll('[data-pick]').forEach((btn) => {
    btn.addEventListener('click', () => pick(btn));
    btn.addEventListener('focus', () => pick(btn));
    btn.addEventListener('pointerenter', (e) => { if (e.pointerType === 'mouse') pick(btn); });
  });
})();

// Entrances: each section's parts rise once, staggered; never left hidden if the observer never fires.
(() => {
  const groups = [...document.querySelectorAll('section, .cta, .flow__field')];
  groups.forEach((g) => g.querySelectorAll(':scope .rise, :scope .show__names li').forEach((el, i) => el.style.setProperty('--i', i)));
  document.querySelectorAll('.flow__field').forEach((f, i) => f.style.setProperty('--i', i));
  const targets = [...document.querySelectorAll('.rise, .show, .flow__field')];
  const show = (el) => el.classList.add('is-in');
  if (!('IntersectionObserver' in window)) return targets.forEach(show);
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => { if (e.isIntersecting) { show(e.target); io.unobserve(e.target); } });
  }, { rootMargin: '0px 0px -10% 0px', threshold: 0.08 });
  targets.forEach((el) => io.observe(el));
  addEventListener('load', () => setTimeout(() => targets.forEach((el) => {
    const r = el.getBoundingClientRect();
    if (r.top < innerHeight && r.bottom > 0) show(el);
  }), 2500));
})();

// The showcase screen rises a little as it scrolls through, over the head behind it.
(() => {
  const stage = document.querySelector('.show__stage');
  if (!stage || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  let raf = 0;
  const update = () => {
    raf = 0;
    const r = stage.getBoundingClientRect();
    const p = Math.min(1, Math.max(0, (innerHeight - r.top) / (innerHeight + r.height)));
    stage.style.setProperty('--lift', `${(0.5 - p) * 48}px`);
  };
  addEventListener('scroll', () => { if (!raf) raf = requestAnimationFrame(update); }, { passive: true });
  update();
})();
