(() => {
  if (location.origin !== 'https://listen.localhost' || location.pathname !== '/index.html') return;
  const pending = new Map();
  let nextId = 0;
  const api = new Proxy(Object.create(null), {
    get(_, method) {
      if (typeof method !== 'string' || ['then', 'constructor', '__proto__'].includes(method)) return undefined;
      return (...args) => new Promise((resolve, reject) => {
        const id = ++nextId;
        const timeoutMs = method === 'check_model_updates' ? 10000 : 4500;
        const timer = setTimeout(() => { pending.delete(id); reject(new Error('Listen did not respond')); }, timeoutMs);
        pending.set(id, { resolve, reject, timer });
        try { window.ipc.postMessage(JSON.stringify({ id, method, args })); }
        catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
      });
    },
  });
  window.pywebview = Object.freeze({ api });
  window.receiveListenResponse = ({ id, value }) => {
    const request = pending.get(id);
    if (!request) return;
    pending.delete(id);
    clearTimeout(request.timer);
    request.resolve(value);
  };

  const initial = window.listenDashboard;
  let appearance = initial.appearance;
  function paintAppearance() {
    const root = document.documentElement;
    root.dataset.theme = appearance.theme;
    root.dataset.layout = appearance.layout;
    root.dataset.mode = appearance.dark ? 'dark' : 'light';
    root.style.colorScheme = appearance.dark ? 'dark' : 'light';
    for (const [name, color] of Object.entries(appearance.colors)) root.style.setProperty(`--${name}`, color);
  }
  if (document.documentElement) paintAppearance();
  document.addEventListener('DOMContentLoaded', () => {
    const swatches = document.querySelector('#appearance-swatches');
    const layouts = document.querySelector('#appearance-layouts');
    const blurb = document.querySelector('#appearance-layout-blurb');
    const themeName = document.querySelector('#appearance-theme-name');
    const prevTheme = document.querySelector('#appearance-theme-prev');
    const nextTheme = document.querySelector('#appearance-theme-next');
    const message = document.querySelector('#appearance-status');
    const defaultBlurbs = {
      desk: 'Tabs along the top.',
      sidebar: 'Pages on the left.',
      compact: 'Small, one column.',
    };
    const layoutWireframe = (key) => {
      if (key === 'sidebar') {
        return `<svg class="layout-wire" viewBox="0 0 88 54" aria-hidden="true">
          <rect class="lw-frame" x="1" y="1" width="86" height="52" rx="6"/>
          <rect class="lw-rail" x="2" y="2" width="22" height="50" rx="5"/>
          <rect class="lw-accent" x="6" y="8" width="14" height="4" rx="2"/>
          <rect class="lw-muted" x="6" y="16" width="12" height="3" rx="1.5"/>
          <rect class="lw-muted" x="6" y="23" width="14" height="3" rx="1.5"/>
          <rect class="lw-muted" x="6" y="30" width="10" height="3" rx="1.5"/>
          <rect class="lw-card" x="30" y="9" width="50" height="16" rx="3"/>
          <rect class="lw-muted" x="30" y="31" width="38" height="4" rx="2"/>
          <rect class="lw-muted" x="30" y="39" width="28" height="4" rx="2"/>
        </svg>`;
      }
      if (key === 'compact') {
        return `<svg class="layout-wire" viewBox="0 0 88 54" aria-hidden="true">
          <rect class="lw-frame" x="18" y="1" width="52" height="52" rx="6"/>
          <rect class="lw-rail" x="19" y="2" width="50" height="10" rx="5"/>
          <rect class="lw-accent" x="24" y="5.5" width="11" height="3" rx="1.5"/>
          <rect class="lw-muted" x="38" y="5.5" width="9" height="3" rx="1.5"/>
          <rect class="lw-muted" x="50" y="5.5" width="9" height="3" rx="1.5"/>
          <rect class="lw-card" x="24" y="17" width="40" height="11" rx="3"/>
          <rect class="lw-card" x="24" y="32" width="40" height="11" rx="3"/>
        </svg>`;
      }
      return `<svg class="layout-wire" viewBox="0 0 88 54" aria-hidden="true">
        <rect class="lw-frame" x="1" y="1" width="86" height="52" rx="6"/>
        <rect class="lw-rail" x="2" y="2" width="84" height="12" rx="5"/>
        <rect class="lw-accent" x="8" y="6" width="14" height="4" rx="2"/>
        <rect class="lw-muted" x="26" y="6.5" width="12" height="3" rx="1.5"/>
        <rect class="lw-muted" x="42" y="6.5" width="12" height="3" rx="1.5"/>
        <rect class="lw-muted" x="58" y="6.5" width="12" height="3" rx="1.5"/>
        <rect class="lw-card" x="10" y="20" width="68" height="14" rx="3"/>
        <rect class="lw-muted" x="10" y="39" width="48" height="4" rx="2"/>
      </svg>`;
    };
    const renderSwatches = () => {
      if (!swatches) return;
      const paletteMap = appearance.swatches || {};
      swatches.replaceChildren(...appearance.themes.map(([key, label]) => {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'theme-swatch';
        button.dataset.themeChoice = key;
        button.title = label;
        button.setAttribute('aria-pressed', String(appearance.theme === key));
        const bar = document.createElement('span');
        bar.className = 'theme-swatch-bar';
        bar.setAttribute('aria-hidden', 'true');
        const colors = paletteMap[key] || ['#211f1d', '#2d2925', '#dcc09a', '#f1e9dd'];
        const bands = colors.length >= 3 ? [colors[0], colors[1], colors[2]] : colors.slice(0, 3);
        for (const hex of bands) {
          const band = document.createElement('i');
          band.style.background = hex;
          bar.append(band);
        }
        const name = document.createElement('span');
        name.className = 'theme-swatch-name';
        name.textContent = label;
        button.append(bar, name);
        button.onclick = () => change('theme', key);
        return button;
      }));
    };
    const renderLayouts = () => {
      if (!layouts) return;
      const blurbMap = appearance.layout_blurbs || {};
      layouts.replaceChildren(...appearance.layouts.map(([key, label]) => {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'layout-card';
        button.dataset.layoutChoice = key;
        button.setAttribute('aria-pressed', String(appearance.layout === key));
        button.innerHTML = layoutWireframe(key);
        const title = document.createElement('span');
        title.className = 'layout-card-title';
        title.textContent = label;
        const caption = document.createElement('span');
        caption.className = 'layout-card-caption';
        caption.textContent = blurbMap[key] || defaultBlurbs[key] || '';
        button.append(title, caption);
        button.onclick = () => change('layout', key);
        return button;
      }));
    };
    const sync = () => {
      paintAppearance();
      if (swatches) {
        for (const button of swatches.querySelectorAll('.theme-swatch')) {
          button.setAttribute('aria-pressed', String(button.dataset.themeChoice === appearance.theme));
        }
      }
      if (themeName && Array.isArray(appearance.themes) && appearance.themes.length) {
        const idx = Math.max(0, appearance.themes.findIndex(([key]) => key === appearance.theme));
        const label = appearance.themes[idx]?.[1] || appearance.theme;
        themeName.textContent = `${label} · ${idx + 1}/${appearance.themes.length}`;
      }
      if (layouts) {
        for (const button of layouts.querySelectorAll('.layout-card')) {
          button.setAttribute('aria-pressed', String(button.dataset.layoutChoice === appearance.layout));
        }
      }
      if (blurb) {
        blurb.textContent = appearance.layout_blurbs?.[appearance.layout] || defaultBlurbs[appearance.layout] || '';
      }
    };
    const stepTheme = (delta) => {
      if (!Array.isArray(appearance.themes) || !appearance.themes.length) return;
      const cur = Math.max(0, appearance.themes.findIndex(([key]) => key === appearance.theme));
      const next = (cur + delta + appearance.themes.length) % appearance.themes.length;
      change('theme', appearance.themes[next][0]);
    };
    if (prevTheme) prevTheme.onclick = () => stepTheme(-1);
    if (nextTheme) nextTheme.onclick = () => stepTheme(1);
    let saving = false;
    async function change(kind, value) {
      if ((kind === 'theme' && value === appearance.theme) || (kind === 'layout' && value === appearance.layout)) return;
      if (saving) { sync(); return; }
      saving = true;
      message.hidden = true;
      try {
        const updated = await api.set_appearance(kind, value);
        if (!updated) throw new Error('save failed');
        appearance = updated;
      } catch {
        message.textContent = 'Listen could not save that appearance setting.';
        message.hidden = false;
      } finally {
        sync();
        saving = false;
      }
    }
    renderSwatches();
    renderLayouts();
    sync();
    document.querySelector('#appearance').hidden = false;
    if (initial.page !== 'dictate') {
      const page = document.querySelector(`.rail-btn[data-page="${initial.page}"]`);
      page?.click();
      document.activeElement?.blur();
    }
    api.__dashboard_ready().catch(() => {});
  }, { once: true });

  const reportDraft = () => {
    const focused = document.hasFocus() && document.activeElement?.id === 'draft';
    api.__draft_focus(focused).catch(() => {});
  };
  window.addEventListener('focus', reportDraft, true);
  window.addEventListener('blur', () => queueMicrotask(reportDraft), true);
  document.addEventListener('input', (event) => {
    if (event.target.id === 'draft') api.__draft_changed().catch(() => {});
  }, true);
})();
