const GH = 'https://api.github.com/repos/lucasAguiar11/yuki';
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
// Yuki's own shortcuts use ⌘; elsewhere the page answers to Ctrl as well.
const mod = (e) => e.metaKey || (!isMac && e.ctrlKey);

// The request: GH is a variable chip, like in the app; the path after it is yours to type.
const req = (() => {
  const section = document.querySelector('[data-req]');
  const form = section.querySelector('[data-live]');
  const input = form.querySelector('.req__path');
  const status = form.querySelector('[data-status]');
  const body = form.querySelector('[data-body]');
  const caption = document.querySelector('[data-caption]');
  const heroStatus = document.querySelector('[data-hero-status]');
  const tries = [...form.querySelectorAll('[data-path]')];
  let busy = false;

  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const size = (n) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`);
  // Pretty JSON with the app's token colours; long bodies stop after 60 lines.
  const paint = (value) => {
    const lines = JSON.stringify(value, null, 2).split('\n');
    const cut = lines.length > 60 ? [...lines.slice(0, 60), `… ${lines.length - 60} more lines`] : lines;
    return cut.map((l) => esc(l)
      .replace(/^(\s*)(&quot;.*?&quot;)(: )/, '$1<span class="k">$2</span><span class="p">$3</span>')
      .replace(/(<\/span>|^\s*)(&quot;.*&quot;)(,?)$/, '$1<span class="s">$2</span>$3')
      .replace(/(<\/span>|^\s*)(-?\d[\d.eE+-]*|true|false|null)(,?)$/, '$1<span class="n">$2</span>$3')).join('\n');
  };

  async function send() {
    if (busy) return;
    busy = true;
    const path = input.value.trim().replace(/^\/+/, '');
    tries.forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.path === path)));
    status.textContent = 'sending…';
    heroStatus.textContent = 'sending…';
    const t0 = performance.now();
    try {
      const res = await fetch(path ? `${GH}/${path}` : GH, { headers: { Accept: 'application/vnd.github+json' } });
      const text = await res.text();
      const ms = Math.round(performance.now() - t0);
      const clock = new Date().toLocaleTimeString([], { hour12: false });
      const head = `<span class="${res.ok ? 'ok' : 'err'}">${res.status} ${esc(res.statusText || (res.ok ? 'OK' : ''))}</span>`;
      status.innerHTML = `${head}  ${ms} ms  ${size(new Blob([text]).size)}  ${clock}`;
      heroStatus.innerHTML = `${head} in ${ms} ms. <a href="#request">See the response</a>`;
      let json = null;
      try { json = JSON.parse(text); } catch { /* not JSON */ }
      body.innerHTML = json === null ? esc(text) : paint(json);
      body.classList.add('t');
      if (res.ok) {
        document.body.setAttribute('data-awake', '');
        caption.textContent = 'Yours came back.';
      }
      if (res.ok && json && json.tag_name) {
        document.querySelectorAll('[data-download]').forEach((a) => { a.textContent = `Download ${json.tag_name}`; });
      }
    } catch (err) {
      status.innerHTML = '<span class="err">× send failed</span>';
      heroStatus.innerHTML = '<span class="err">× send failed</span> <a href="#request">See why</a>';
      body.textContent = `${err.message}\nThe browser stops requests another site hasn't allowed (CORS). Yuki is a native app and has no such limit.`;
    } finally {
      busy = false;
    }
  }

  form.addEventListener('submit', (e) => { e.preventDefault(); send(); });
  tries.forEach((b) => b.addEventListener('click', () => { input.value = b.dataset.path; send(); }));
  return { send };
})();

// ⌘↵ anywhere sends. The hero shows the result next to Yuki; the full response waits below.
const goSend = () => req.send();
document.querySelectorAll('[data-go-send]').forEach((b) => b.addEventListener('click', goSend));
addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); goSend(); }
});

// The hero's loop: type a request, press ⌘↵, watch the response land, and Yuki wakes for it.
(() => {
  const visual = document.querySelector('[data-visual]');
  const sim = visual.querySelector('[data-sim]');
  if (reduced) return; // the static, finished send in the markup stays
  const $ = (k) => sim.querySelector(`[data-sim-${k}]`);
  const [method, url, meta, status, body] = ['method', 'url', 'meta', 'status', 'body'].map($);
  const k = (s) => `<span class="k">"${s}"</span><span class="p">:</span> `;
  const sends = [
    { m: 'GET', chip: 'API', path: '/users/1', meta: 'headers 1 · body — · auth bearer', st: '200 OK', ms: 21, size: '509 B',
      lines: ['{', `  ${k('id')}<span class="n">1</span>,`, `  ${k('name')}<span class="s">"Leanne Graham"</span>,`, `  ${k('email')}<span class="s">"Sincere@april.biz"</span>`, '}'] },
    { m: 'POST', chip: 'API', path: '/users', meta: 'headers 1 · body json · auth bearer', st: '201 Created', ms: 48, size: '96 B',
      lines: ['{', `  ${k('id')}<span class="n">11</span>,`, `  ${k('name')}<span class="s">"Rafael Martins"</span>,`, `  ${k('email')}<span class="s">"hugo64@example.com"</span>`, '}'] },
    { m: 'GET', chip: 'GH', path: '/releases/latest', meta: 'headers 1 · body — · auth —', st: '200 OK', ms: 312, size: '11.9 KB',
      lines: ['{', `  ${k('tag_name')}<span class="s">"v0.4.0"</span>,`, `  ${k('name')}<span class="s">"v0.4.0"</span>,`, `  ${k('draft')}<span class="n">false</span>`, '}'] },
  ];
  let visible = true;
  new IntersectionObserver(([e]) => { visible = e.isIntersecting; }).observe(visual);
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const idle = async () => { while (!visible || document.hidden) await wait(400); };

  (async () => {
    await wait(1600);
    for (let i = 0; ; i = (i + 1) % sends.length) {
      const s = sends[i];
      await idle();
      visual.removeAttribute('data-awake');
      method.textContent = s.m;
      meta.textContent = s.meta;
      status.textContent = 'no response';
      body.textContent = '';
      url.innerHTML = `<span class="chip">${s.chip}</span>`;
      sim.setAttribute('data-typing', '');
      for (const ch of s.path) { url.append(ch); await wait(70 + Math.random() * 60); }
      await wait(500);
      sim.removeAttribute('data-typing');
      sim.setAttribute('data-pressed', '');
      await wait(180);
      sim.removeAttribute('data-pressed');
      status.textContent = 'sending…';
      await wait(Math.min(900, 300 + s.ms));
      status.innerHTML = `<span class="ok">${s.st}</span>  ${s.ms} ms  ${s.size}`;
      visual.setAttribute('data-awake', '');
      for (const line of s.lines) { body.insertAdjacentHTML('beforeend', `${line}\n`); await wait(90); }
      await wait(3200);
    }
  })();
})();

// Keys: the pinned screen follows the scroll; pressing a shortcut scrolls to its step.
(() => {
  const section = document.querySelector('[data-keys]');
  const buttons = [...section.querySelectorAll('[data-k]')];
  const screens = [...section.querySelectorAll('[data-screen]')];
  const now = section.querySelector('[data-now]');
  let current = 0;
  let inView = false;

  const show = (i) => {
    if (i === current) return;
    current = i;
    buttons.forEach((b, j) => b.setAttribute('aria-pressed', String(j === i)));
    screens.forEach((img) => img.classList.toggle('is-on', img.dataset.screen === buttons[i].dataset.shot));
    now.textContent = `${buttons[i].querySelector('kbd').textContent} · ${buttons[i].querySelector('span').textContent}`;
  };
  const stepHeight = () => (section.offsetHeight - innerHeight) / buttons.length;
  const go = (i) => {
    const btn = buttons[i];
    btn.classList.add('is-hit');
    setTimeout(() => btn.classList.remove('is-hit'), 140);
    show(i);
    const y = section.getBoundingClientRect().top + scrollY + stepHeight() * (i + 0.5);
    scrollTo({ top: y, behavior: reduced ? 'auto' : 'smooth' });
  };
  buttons.forEach((b, i) => b.addEventListener('click', () => go(i)));

  let raf = 0;
  addEventListener('scroll', () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      const r = section.getBoundingClientRect();
      inView = r.top < innerHeight * 0.5 && r.bottom > innerHeight * 0.5;
      if (r.top > 0 || r.bottom < innerHeight) return;
      show(Math.min(buttons.length - 1, Math.floor(-r.top / stepHeight())));
    });
  }, { passive: true });

  const combo = (e) => `${e.altKey ? 'alt+' : ''}${e.shiftKey ? 'shift+' : ''}${e.code.replace('Key', '')}`;
  addEventListener('keydown', (e) => {
    if (!inView || !mod(e) || e.key === 'Enter') return;
    const i = buttons.findIndex((b) => b.dataset.k === combo(e));
    if (i < 0) return;
    e.preventDefault();
    go(i);
  });
})();

// Entrances: each section's parts rise once, staggered; nothing stays hidden if the observer never fires.
(() => {
  document.querySelectorAll('section').forEach((s) => s.querySelectorAll('.rise').forEach((el, i) => el.style.setProperty('--i', i)));
  const targets = [...document.querySelectorAll('.rise')];
  const show = (el) => el.classList.add('is-in');
  if (!('IntersectionObserver' in window)) return targets.forEach(show);
  const io = new IntersectionObserver((entries) => entries.forEach((e) => {
    if (e.isIntersecting) { show(e.target); io.unobserve(e.target); }
  }), { rootMargin: '0px 0px -10% 0px', threshold: 0.08 });
  targets.forEach((el) => io.observe(el));
  addEventListener('load', () => setTimeout(() => targets.forEach((el) => {
    const r = el.getBoundingClientRect();
    if (r.top < innerHeight && r.bottom > 0) show(el);
  }), 2500));
})();
