'use strict';
// Everwall's settings window. It asks the app for its state (invoke) and
// draws it; every change is saved at once, with no Save button. Opened in an
// ordinary browser (no Tauri) it uses sample data, so it can be reviewed.
// Every visible text comes in both languages: tr('español', 'English').

const TAURI = window.__TAURI__;
const invoke = TAURI ? TAURI.core.invoke : demoInvoke;
const listen = TAURI ? TAURI.event.listen : async () => {};

// index.html is written in English; translateStatic() switches it to Spanish.
let LANG = 'en';
const tr = (es, en) => (LANG === 'en' ? en : es);

const EVERY = () => [
  ['5', tr('Cada 5 minutos', 'Every 5 minutes')],
  ['15', tr('Cada 15 minutos', 'Every 15 minutes')],
  ['30', tr('Cada 30 minutos', 'Every 30 minutes')],
  ['60', tr('Cada hora', 'Every hour')],
  ['180', tr('Cada 3 horas', 'Every 3 hours')],
  ['360', tr('Cada 6 horas', 'Every 6 hours')],
  ['daily', tr('Una vez al día', 'Once a day')],
  ['start', tr('Sólo al encender la PC', 'Only when the PC starts')],
  ['manual', tr('Sólo cuando yo lo pida', 'Only when I ask')],
];
const FIT = () => ({
  auto: tr('Automática (recomendada)', 'Automatic (recommended)'),
  fill: tr('Llenar la pantalla (recorta)', 'Fill the screen (crops)'),
  blur: tr('Imagen entera con fondo difuminado', 'Whole image on a blurred background'),
  triptych: tr('Tríptico: tres imágenes juntas', 'Triptych: three images together'),
});
const FIT_HELP = () => ({
  auto: tr('Cada imagen se coloca según su forma: si encaja, llena la pantalla; si tiene la otra forma, se juntan tres en un tríptico (verticales lado a lado en una pantalla ancha, horizontales apiladas en una vertical).',
    'Each image is placed by its shape: if it fits, it fills the screen; if it has the other shape, three go together in a triptych (portraits side by side on a wide screen, landscapes stacked on a tall one).'),
  fill: tr('Elige imágenes con la misma forma que la pantalla y las recorta lo justo para llenarla.',
    'Picks images with the same shape as the screen and crops just enough to fill it.'),
  blur: tr('Siempre la imagen entera, sin recortar, sobre una copia difuminada de sí misma.',
    'Always the whole image, uncropped, over a blurred copy of itself.'),
  triptych: tr('Siempre tres imágenes de la otra forma: verticales lado a lado en una pantalla ancha, horizontales apiladas en una vertical.',
    'Always three images of the other shape: portraits side by side on a wide screen, landscapes stacked on a tall one.'),
});
const LAYOUT = () => ({
  fill: tr('Pantalla completa', 'Full screen'),
  blur: tr('Entera, con difuminado', 'Whole, blurred behind'),
  triptych: tr('Tríptico', 'Triptych'),
});
const SLOT = () => ({ morning: tr('mañana', 'morning'), afternoon: tr('tarde', 'afternoon'), night: tr('noche', 'night') });
const SLOTS = ['morning', 'afternoon', 'night'];
// GNOME already uses Ctrl + Alt + Shift + arrows, and it does not tell apps
// where the mouse is: on Linux the shortcuts act on the main screen.
const LINUX = () => S && S.system === 'linux';
const KEYS = () => (LINUX() ? 'Super + Ctrl + Alt' : tr('Ctrl + Alt + Mayús', 'Ctrl + Alt + Shift'));
const HOTKEYS = () => {
  const keys = KEYS();
  return [
    [`${keys} + →`, LINUX()
      ? tr('Siguiente fondo en la pantalla principal', 'Next wallpaper on the main screen')
      : tr('Siguiente fondo en la pantalla del ratón', 'Next wallpaper on the screen under the mouse')],
    [`${keys} + ←`, tr('Volver al fondo anterior en esa pantalla', 'Back to the previous wallpaper on that screen')],
    [`${keys} + ↑`, tr('Marcar como favorito (con tres imágenes, abre la ventana para elegir)', 'Mark as favourite (with three images, opens the window to choose)')],
    [`${keys} + ↓`, tr('No volver a mostrarlo (con tres imágenes, abre la ventana para elegir)', 'Never show it again (with three images, opens the window to choose)')],
  ];
};

let S = null;
let view = 'screens';
let selScreen = null;
let selColl = null;
const previews = new Map();
const counts = new Map();

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'svg') el.innerHTML = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k in el && typeof v !== 'string') el[k] = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}
const $ = (id) => document.getElementById(id);
const cap = (t) => t.charAt(0).toUpperCase() + t.slice(1);
const base = (p) => p.split(/[\\/]/).pop();
const hour12 = (hr) => `${hr % 12 || 12}:00 ${hr < 12 ? tr('a. m.', 'AM') : tr('p. m.', 'PM')}`;

const ICON = {
  hide: '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3 3l18 18"/><path d="M10.6 6.1A9.8 9.8 0 0 1 12 6c5 0 9 6 9 6a16 16 0 0 1-2.6 3.2M6.1 7.6C4.1 9.1 3 12 3 12s4 6 9 6a8.7 8.7 0 0 0 4-1"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/></svg>',
  folder: '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
};

/** The texts written in index.html carry their Spanish in data-es. */
function translateStatic() {
  document.documentElement.lang = LANG;
  for (const el of document.querySelectorAll('[data-es]')) {
    if (!el.dataset.en) el.dataset.en = el.textContent;
    el.textContent = LANG === 'en' ? el.dataset.en : el.dataset.es;
  }
  for (const el of document.querySelectorAll('[data-title-es]')) {
    if (!el.dataset.titleEn) el.dataset.titleEn = el.title;
    el.title = LANG === 'en' ? el.dataset.titleEn : el.dataset.titleEs;
  }
}

function toast(message) {
  const t = $('toast');
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { t.hidden = true; }, 2800);
}

// The collections Everwall created itself keep their English names on disk;
// shown in Spanish while the user has not renamed them.
function displayName(c) {
  if (LANG === 'en') return c.name;
  if (c.id === 'all' && c.name === 'All my wallpapers') return 'Todos mis fondos';
  if (c.name === 'New collection') return 'Colección nueva';
  const folder = /^Folder (.+)$/.exec(c.name);
  return folder ? `Carpeta ${folder[1]}` : c.name;
}

function collName(id) {
  if (id === 'favorites') return tr('★ Favoritos', '★ Favourites');
  const c = S.collections.find((x) => x.id === id);
  return c ? displayName(c) : id;
}

function position(p) {
  const n = S.screens.length;
  const i = S.screens.indexOf(p);
  if (n === 2) return [tr('izquierda', 'left'), tr('derecha', 'right')][i];
  if (n === 3) return [tr('izquierda', 'left'), tr('centro', 'centre'), tr('derecha', 'right')][i];
  return n > 1 ? tr(`${i + 1}.ª desde la izquierda`, `#${i + 1} from the left`) : '';
}

// Two identical monitors share a name; their position tells them apart.
function fullName(p) {
  const twins = S.screens.filter((q) => q.name === p.name).length > 1;
  return twins ? `${p.name} (${position(p)})` : p.name;
}

const describe = (p) => `${p.w}×${p.h} · ${p.w >= p.h ? tr('horizontal', 'landscape') : tr('vertical', 'portrait')}${p.primary ? tr(' · principal', ' · main') : ''}`;

let refreshTimer = null;
function refreshSoon() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refresh, 120);
}

async function refresh() {
  S = await invoke('state');
  if (S.general.language !== LANG) {
    LANG = S.general.language === 'en' ? 'en' : 'es';
    translateStatic();
  }
  if (S.focus) {
    selScreen = S.focus;
    show('screens');
  }
  if (!S.screens.some((p) => p.id === selScreen)) {
    selScreen = (S.screens.find((p) => p.primary) || S.screens[0] || {}).id;
  }
  if (selColl !== 'favorites' && !S.collections.some((c) => c.id === selColl)) {
    selColl = S.collections[0] && S.collections[0].id;
  }
  renderFoot();
  renderScreens();
  renderCollections();
  renderGeneral();
}

function renderFoot() {
  const state = S.busy
    ? ['busy', tr('Preparando fondo…', 'Preparing wallpaper…')]
    : S.paused ? ['paused', tr('En pausa', 'Paused')] : ['', tr('Cambiando solo', 'Changing on its own')];
  $('status').replaceChildren(
    h('span', { class: 'pill' }, h('span', { class: `dot ${state[0]}` }), state[1]),
    h('span', {}, `${tr('Ahora', 'Now')}: ${SLOT()[S.slot]}`),
    h('span', {}, `${S.favorites} ${S.favorites === 1 ? tr('favorito', 'favourite') : tr('favoritos', 'favourites')}`),
  );
  $('pause').checked = S.paused;
}

/* ---------- Screens ---------- */

function renderScreens() {
  const error = $('error');
  error.hidden = !S.error;
  error.textContent = S.error || '';
  renderMap();
  renderDetail();
}

function renderMap() {
  const map = $('map');
  const ps = S ? S.screens : [];
  if (!ps.length) {
    map.style.width = map.style.height = '';
    map.replaceChildren(h('p', { class: 'muted' }, tr('Buscando pantallas…', 'Looking for screens…')));
    return;
  }
  const minX = Math.min(...ps.map((p) => p.x));
  const minY = Math.min(...ps.map((p) => p.y));
  const width = Math.max(...ps.map((p) => p.x + p.w)) - minX;
  const height = Math.max(...ps.map((p) => p.y + p.h)) - minY;
  const room = map.parentElement.clientWidth - 44;
  const k = Math.min((room > 0 ? room : 700) / width, 240 / height);
  const gap = 6;
  map.style.width = `${width * k}px`;
  map.style.height = `${height * k}px`;
  map.replaceChildren(...ps.map((p) => {
    const el = h('div', {
      class: `mon${p.id === selScreen ? ' sel' : ''}`,
      title: `${fullName(p)} · ${describe(p)}`,
      onclick: () => { selScreen = p.id; renderScreens(); },
    },
    p.current && p.current.mini ? h('img', { src: p.current.mini, alt: '' }) : h('div', { class: 'empty' }),
    p.primary ? h('span', { class: 'badge' }, tr('Principal', 'Main')) : null,
    h('span', { class: 'tag' }, fullName(p)));
    Object.assign(el.style, {
      left: `${(p.x - minX) * k + gap / 2}px`,
      top: `${(p.y - minY) * k + gap / 2}px`,
      width: `${p.w * k - gap}px`,
      height: `${p.h * k - gap}px`,
    });
    return el;
  }));
}

function collectionOptions(value) {
  return [
    ...S.collections.map((c) => h('option', { value: c.id, selected: c.id === value }, displayName(c))),
    h('option', { value: 'favorites', selected: value === 'favorites' }, collName('favorites')),
  ];
}

function switchRow(text, checked, onchange, help) {
  return h('label', { class: 'row', style: 'cursor:pointer' },
    h('div', {}, h('div', {}, text), help ? h('div', { class: 'muted small' }, help) : null),
    h('span', { class: 'switch' }, h('input', { type: 'checkbox', checked, onchange: (e) => onchange(e.target.checked) }), h('span')));
}

function renderDetail() {
  const box = $('detail');
  const p = S.screens.find((q) => q.id === selScreen);
  if (!p) {
    box.replaceChildren();
    return;
  }
  const a = { ...p.settings };
  const save = async () => {
    await invoke('save_screen', { id: p.id, settings: a });
    refreshSoon();
  };
  const select = (key, options) => h('select', { onchange: (e) => { a[key] = e.target.value; save(); } }, options);
  const fit = FIT();
  const help = FIT_HELP();

  const settings = h('div', { class: 'card' },
    h('div', { class: 'card-head' }, h('h2', {}, fullName(p)), h('span', { class: 'muted small' }, describe(p))),
    switchRow(tr('Distintas según la hora del día', 'Different by time of day'), a.by_time, (on) => { a.by_time = on; save(); },
      tr('Mañana, tarde y noche con su propia colección. Las horas se cambian en General.',
        'Morning, afternoon and night each with their own collection. The hours are set in General.')),
    a.by_time
      ? h('div', { class: 'slots' }, SLOTS.map((s) => h('div', { class: 'field' },
        h('label', {}, `${cap(SLOT()[s])} (${hour12(S.general[`${s}_from`])})`), select(s, collectionOptions(a[s])))))
      : h('div', { class: 'field' }, h('label', {}, tr('Imágenes de', 'Images from')), select('collection', collectionOptions(a.collection))),
    h('div', { class: 'field' }, h('label', {}, tr('Cambiar', 'Change')),
      select('every', EVERY().map(([v, t]) => h('option', { value: v, selected: v === a.every }, t)))),
    h('div', { class: 'field' }, h('label', {}, tr('Colocación', 'Placement')),
      select('fit', Object.entries(fit).map(([v, t]) => h('option', { value: v, selected: v === a.fit }, t))),
      h('p', { class: 'muted small' }, help[a.fit] || '')),
  );

  const current = p.current;
  const source = collName(a.by_time ? a[S.slot] : a.collection);
  const now = h('div', { class: 'card' },
    h('div', { class: 'card-head' }, h('h2', {}, tr('Ahora en pantalla', 'On screen now')),
      current ? h('span', { class: 'layout-name' }, LAYOUT()[current.layout]) : null),
    current
      ? h('div', { class: 'sources' }, current.sources.map(sourceRow))
      : h('p', { class: 'muted' }, S.busy
        ? tr('Preparando el primer fondo…', 'Preparing the first wallpaper…')
        : tr('Todavía no tiene fondo de Everwall.', 'No wallpaper from Everwall yet.')),
    h('div', { class: 'buttons' },
      h('button', { class: 'btn', disabled: !p.back, onclick: () => act('previous', { id: p.id }) }, tr('← Anterior', '← Previous')),
      h('button', { class: 'btn primary', onclick: () => act('next', { id: p.id }) }, tr('Siguiente →', 'Next →'))),
    h('p', { class: 'muted small' }, LINUX()
      ? tr(`De «${source}». ${KEYS()} + → pasa al siguiente en la pantalla principal.`,
        `From “${source}”. ${KEYS()} + → moves to the next one on the main screen.`)
      : tr(`De «${source}». Con el ratón sobre esta pantalla, ${KEYS()} + → pasa al siguiente.`,
        `From “${source}”. With the mouse on this screen, ${KEYS()} + → moves to the next one.`)),
  );
  box.replaceChildren(settings, now);
}

function sourceRow(f) {
  const img = h('img', { class: 'thumb', alt: '' });
  loadPreview(f.path, img);
  return h('div', { class: 'source' },
    img,
    h('div', { class: 'name', title: f.path }, f.name),
    h('div', { class: 'acts' },
      h('button', {
        class: `btn ghost icon star${f.favorite ? ' on' : ''}`,
        title: f.favorite ? tr('Quitar de favoritos', 'Remove from favourites') : tr('Marcar como favorito', 'Mark as favourite'),
        onclick: async () => {
          await invoke('favorite', { path: f.path, on: !f.favorite });
          toast(f.favorite ? tr('Quitada de favoritos', 'Removed from favourites') : tr('Añadida a favoritos', 'Added to favourites'));
          refresh();
        },
      }, f.favorite ? '★' : '☆'),
      h('button', {
        class: 'btn ghost icon', title: tr('No volver a mostrarla', 'Never show it again'), svg: ICON.hide,
        onclick: async () => {
          await invoke('hide', { path: f.path });
          toast(tr('No volverá a salir. Se puede recuperar en General.', 'It will not show again. You can bring it back in General.'));
          refreshSoon();
        },
      }),
      h('button', { class: 'btn ghost icon', title: tr('Ver en su carpeta', 'Show in its folder'), svg: ICON.folder, onclick: () => invoke('reveal', { path: f.path }) }),
    ));
}

async function loadPreview(path, img) {
  if (previews.has(path)) {
    img.src = previews.get(path);
    return;
  }
  const url = await invoke('thumbnail', { path });
  if (url) {
    previews.set(path, url);
    img.src = url;
  }
}

async function act(command, args) {
  await invoke(command, args);
  refreshSoon();
}

/* ---------- Collections ---------- */

function renderCollections() {
  const item = (id, name) => {
    const n = counts.get(id);
    if (!counts.has(id)) loadCount(id);
    return h('button', {
      class: `list-item${id === selColl ? ' sel' : ''}`,
      onclick: () => { selColl = id; renderCollections(); },
    }, h('span', {}, name), h('span', { class: 'count' }, n ? String(n.total) : ''));
  };
  $('collection-list').replaceChildren(
    ...S.collections.map((c) => item(c.id, displayName(c))),
    h('hr'),
    item('favorites', collName('favorites')),
  );
  renderEditor();
}

async function loadCount(id) {
  counts.set(id, null);
  const n = await invoke('count', { id });
  counts.set(id, n);
  if (view === 'collections') renderCollections();
}

function stats(id) {
  const n = counts.get(id);
  const stat = (v, label) => h('div', { class: 'stat' }, h('b', {}, v == null ? '…' : String(v)), h('span', {}, label));
  return h('div', { class: 'stats' },
    stat(n && n.total, tr('imágenes', 'images')),
    stat(n && n.portrait, tr('verticales', 'portrait')),
    stat(n && n.landscape, tr('horizontales', 'landscape')));
}

function usedBy(id) {
  const uses = (a) => a.collection === id || (a.by_time && SLOTS.some((s) => a[s] === id));
  const names = S.screens.filter((p) => uses(p.settings)).map(fullName);
  return h('p', { class: 'muted small' }, names.length
    ? tr(`La usa: ${names.join(', ')}.`, `Used by: ${names.join(', ')}.`)
    : tr('Ninguna pantalla la usa ahora.', 'No screen uses it right now.'));
}

function confirmButton(label, question, onYes, cls) {
  const wrap = h('span', { class: 'buttons' });
  const first = h('button', {
    class: cls || 'btn',
    onclick: () => wrap.replaceChildren(
      h('span', { class: 'muted', style: 'align-self:center' }, question),
      h('button', { class: 'btn danger', onclick: onYes }, tr('Sí', 'Yes')),
      h('button', { class: 'btn', onclick: () => wrap.replaceChildren(first) }, 'No')),
  }, label);
  wrap.append(first);
  return wrap;
}

function renderEditor() {
  const box = $('collection-editor');
  const typing = document.activeElement && box.contains(document.activeElement) && document.activeElement.type === 'text';
  if (typing) return;
  if (selColl === 'favorites') {
    box.replaceChildren(
      h('h2', {}, collName('favorites')),
      h('p', { class: 'muted' }, tr(
        'Las imágenes marcadas con la estrella. Una pantalla puede sacar sus fondos de aquí como de cualquier colección, y en las demás colecciones salen tres veces más a menudo.',
        'The images marked with the star. A screen can take its wallpapers from here like from any collection, and in the other collections they come up three times as often.')),
      stats('favorites'), usedBy('favorites'));
    return;
  }
  const c = S.collections.find((x) => x.id === selColl);
  if (!c) {
    box.replaceChildren(h('p', { class: 'muted' }, tr('Elige una colección.', 'Pick a collection.')));
    return;
  }
  const draft = JSON.parse(JSON.stringify(c));
  const save = async (recount) => {
    await invoke('save_collection', { collection: draft });
    if (recount) counts.delete(draft.id);
    await refresh();
  };
  let nameTimer = null;
  box.replaceChildren(
    h('div', { class: 'field' }, h('label', {}, tr('Nombre', 'Name')),
      h('input', {
        type: 'text', value: displayName(c), maxlength: '60',
        oninput: (e) => {
          draft.name = e.target.value;
          clearTimeout(nameTimer);
          nameTimer = setTimeout(() => invoke('save_collection', { collection: draft }), 400);
        },
        onblur: () => { clearTimeout(nameTimer); if (draft.name !== c.name) save(false); },
        onkeydown: (e) => { if (e.key === 'Enter') e.target.blur(); },
      })),
    h('div', { class: 'field' }, h('span', { class: 'label' }, tr('Carpetas', 'Folders')),
      draft.folders.length
        ? h('div', { class: 'folders' }, draft.folders.map((f, i) => h('div', { class: 'folder' },
          h('div', { style: 'flex:1;min-width:0' }, h('div', {}, base(f) || f), h('div', { class: 'muted small name', title: f }, f)),
          h('button', { class: 'btn ghost icon', title: tr('Quitar esta carpeta', 'Remove this folder'), onclick: () => { draft.folders.splice(i, 1); save(true); } }, '✕'))))
        : h('p', { class: 'muted' }, tr('Todavía no tiene carpetas.', 'No folders yet.')),
      h('div', { class: 'buttons' }, h('button', {
        class: 'btn',
        onclick: async () => {
          const folder = await invoke('pick_folder');
          if (folder && !draft.folders.includes(folder)) {
            draft.folders.push(folder);
            save(true);
          }
        },
      }, tr('Añadir carpeta…', 'Add folder…')))),
    switchRow(tr('Incluir las subcarpetas', 'Include subfolders'), c.recursive, (on) => { draft.recursive = on; save(true); }),
    stats(c.id),
    usedBy(c.id),
    S.collections.length > 1
      ? confirmButton(tr('Borrar colección', 'Delete collection'),
        tr(`¿Borrar «${displayName(c)}»? Las imágenes no se tocan.`, `Delete “${displayName(c)}”? The images are not touched.`),
        async () => {
          try {
            await invoke('delete_collection', { id: c.id });
            counts.delete(c.id);
            selColl = null;
            await refresh();
          } catch (e) {
            toast(String(e));
          }
        }, 'btn danger')
      : null,
  );
}

/* ---------- General ---------- */

function renderGeneral() {
  const g = { ...S.general };
  const save = async () => {
    await invoke('save_general', { general: g });
    refreshSoon();
  };
  const slot = SLOT();
  const hourSelect = (key) => h('select', {
    onchange: (e) => {
      const next = { ...g, [key]: Number(e.target.value) };
      if (!(next.morning_from < next.afternoon_from && next.afternoon_from < next.night_from)) {
        toast(tr('Tienen que ir en orden: mañana, tarde y noche.', 'They have to go in order: morning, afternoon, night.'));
        e.target.value = g[key];
        return;
      }
      g[key] = next[key];
      save();
    },
  }, Array.from({ length: 24 }, (_, i) => h('option', { value: String(i), selected: i === g[key] }, hour12(i))));

  $('general').replaceChildren(
    h('div', { class: 'card' }, h('h2', {}, tr('Idioma', 'Language')),
      h('label', { class: 'row' },
        h('div', {}, h('div', {}, tr('Idioma de Everwall', 'Everwall language')),
          h('div', { class: 'muted small' }, tr('La ventana, el menú del icono junto al reloj y los avisos.', 'The window, the menu of the icon by the clock and the messages.'))),
        h('select', {
          style: 'max-width:200px',
          onchange: (e) => { g.language = e.target.value; save(); },
        },
        h('option', { value: 'es', selected: g.language !== 'en' }, 'Español'),
        h('option', { value: 'en', selected: g.language === 'en' }, 'English')))),
    h('div', { class: 'card' }, h('h2', {}, tr('Funcionamiento', 'Behaviour')),
      switchRow(LINUX() ? tr('Arrancar al iniciar sesión', 'Start when I log in') : tr('Arrancar con Windows', 'Start with Windows'), g.start_with_windows, (on) => { g.start_with_windows = on; save(); },
        tr('Everwall se queda junto al reloj y cambia los fondos aunque esta ventana esté cerrada.',
          'Everwall stays by the clock and changes wallpapers even with this window closed.')),
      switchRow(tr('Esperar si hay un juego o un vídeo a pantalla completa', 'Wait while a game or video is full screen'), g.pause_fullscreen, (on) => { g.pause_fullscreen = on; save(); },
        LINUX()
          ? tr('GNOME no avisa de la pantalla completa: el cambio espera mientras algo pida no apagar la pantalla. Los navegadores lo piden mientras suena un vídeo, y algunas apps todo el rato; por eso viene apagado.',
            'GNOME does not say what is full screen: the change waits while something asks to keep the screen on. Browsers ask while a video plays, and some apps ask all the time, so it starts off.')
          : tr('El cambio se hace al salir de la pantalla completa.', 'The change happens when full screen ends.'))),
    h('div', { class: 'card' }, h('h2', {}, tr('Color de acento', 'Accent colour')),
      LINUX()
        ? switchRow(tr('Tomar el color de acento de GNOME del fondo', 'Take the GNOME accent colour from the wallpaper'), g.accent, (on) => { g.accent = on; save(); },
          tr('GNOME tiene nueve colores de acento: usa el más parecido al color más vivo del fondo. Al apagarlo vuelve el color que tenías.',
            'GNOME has nine accent colours: it uses the one closest to the most vivid colour of the wallpaper. Turning it off brings back the colour you had.'))
        : switchRow(tr('Tomar el color de acento de Windows del fondo', 'Take the Windows accent colour from the wallpaper'), g.accent, (on) => { g.accent = on; save(); },
          tr('Usa el color más vivo del fondo. Al apagarlo vuelve el color que tenías. Cada cambio de color hace que Windows repinte todas las ventanas durante uno o dos minutos, por eso no cambia con cada fondo.',
            'Uses the most vivid colour of the wallpaper. Turning it off brings back the colour you had. Each colour change makes Windows repaint every window for a minute or two, so it does not change with every wallpaper.')),
      g.accent ? h('label', { class: 'row' }, h('span', {}, tr('Del fondo de', 'From the wallpaper of')),
        h('select', { style: 'max-width:280px', onchange: (e) => { g.accent_screen = e.target.value; save(); } },
          h('option', { value: '', selected: !g.accent_screen }, tr('La pantalla principal', 'The main screen')),
          S.screens.map((p) => h('option', { value: p.id, selected: g.accent_screen === p.id }, fullName(p))))) : null,
      g.accent ? h('label', { class: 'row' }, h('span', {}, tr('Cambiar el color como mucho', 'Change the colour at most')),
        h('select', { style: 'max-width:280px', onchange: (e) => { g.accent_every = Number(e.target.value); save(); } },
          [[1, tr('Cada hora', 'Every hour')], [3, tr('Cada 3 horas', 'Every 3 hours')], [6, tr('Cada 6 horas', 'Every 6 hours')], [24, tr('Una vez al día', 'Once a day')]]
            .map(([v, t]) => h('option', { value: String(v), selected: v === g.accent_every }, t)))) : null),
    h('div', { class: 'card' }, h('h2', {}, tr('Horas del día', 'Times of day')),
      h('p', { class: 'muted small' }, tr(
        `Para las pantallas con fondos distintos según la hora. Ahora es de ${slot[S.slot]}.`,
        `For screens with different wallpapers by time of day. It is ${slot[S.slot]} now.`)),
      h('div', { class: 'slots' }, SLOTS.map((s) => h('div', { class: 'field' },
        h('label', {}, tr(`${cap(slot[s])} desde`, `${cap(slot[s])} from`)), hourSelect(`${s}_from`))))),
    h('div', { class: 'card' }, h('h2', {}, tr('Atajos de teclado', 'Keyboard shortcuts')),
      h('p', { class: 'muted small' }, LINUX()
        ? tr('Funcionan en cualquier momento, sobre la pantalla principal: GNOME no dice a las apps dónde está el ratón. Están en Configuración › Teclado › Atajos personalizados.',
          'They work at any time, on the main screen: GNOME does not tell apps where the mouse is. They are in Settings › Keyboard › Custom Shortcuts.')
        : tr('Funcionan en cualquier momento, sobre la pantalla donde está el ratón.',
          'They work at any time, on the screen where the mouse is.')),
      h('div', { class: 'keys' }, HOTKEYS().map(([keys, what], i) => [
        h('kbd', {}, keys), h('span', {}, what),
        S.hotkeys[i]
          ? h('span', { class: 'ok' }, tr('Activo', 'Active'))
          : h('span', { class: 'bad' }, tr('Lo usa otra app', 'Taken by another app')),
      ]))),
    h('div', { class: 'card' }, h('h2', {}, tr(`Imágenes ocultas (${S.hidden.length})`, `Hidden images (${S.hidden.length})`)),
      S.hidden.length
        ? h('div', { class: 'hidden-list' }, S.hidden.map((r) => h('div', { class: 'row' },
          h('span', { class: 'name', title: r }, base(r)),
          h('button', {
            class: 'btn',
            onclick: async () => {
              await invoke('unhide', { path: r });
              toast(tr('Vuelve a salir en sus colecciones.', 'It shows up again in its collections.'));
              refresh();
            },
          }, tr('Volver a mostrar', 'Show again')))))
        : h('p', { class: 'muted' }, tr('Ninguna. Las que ocultes desde Pantallas aparecen aquí.', 'None. The ones you hide from Screens show up here.'))),
    S.original ? h('div', { class: 'card' }, h('h2', {}, tr('Volver a como estaba', 'Back to how it was')),
      h('p', { class: 'muted small' }, tr(
        'Pone en cada pantalla el fondo que tenía antes de Everwall, devuelve tu color de acento y deja Everwall en pausa.',
        'Puts back the wallpaper each screen had before Everwall, restores your accent colour and pauses Everwall.')),
      confirmButton(tr('Poner mis fondos de antes', 'Restore my previous wallpapers'), tr('¿Seguro?', 'Are you sure?'), async () => {
        await invoke('restore_original');
        toast(tr('Hecho. Everwall queda en pausa.', 'Done. Everwall is paused.'));
        refreshSoon();
      })) : null,
  );
}

/* ---------- Start ---------- */

function show(next) {
  view = next;
  for (const b of document.querySelectorAll('.nav-item')) b.classList.toggle('active', b.dataset.view === next);
  for (const s of document.querySelectorAll('.view')) s.hidden = s.id !== `v-${next}`;
  if (!S) return;
  if (next === 'screens') renderMap();
  if (next === 'collections') {
    counts.clear();
    renderCollections();
  }
}

for (const b of document.querySelectorAll('.nav-item')) b.addEventListener('click', () => show(b.dataset.view));
$('pause').addEventListener('change', (e) => act('pause', { on: e.target.checked }));
$('next-all').addEventListener('click', () => act('next', { id: null }));
$('new-collection').addEventListener('click', async () => {
  const c = await invoke('new_collection');
  selColl = c.id;
  await refresh();
  show('collections');
  const input = document.querySelector('#collection-editor input[type=text]');
  if (input) { input.focus(); input.select(); }
});
window.addEventListener('resize', () => { if (S && view === 'screens') renderMap(); });
listen('changed', refreshSoon);
listen('focus', (e) => {
  if (e.payload) selScreen = e.payload;
  show('screens');
  refresh();
});
refresh();

/* ---------- Sample data (only outside the app) ---------- */

function demoInvoke(command, args) {
  const D = demoInvoke.data || (demoInvoke.data = demoData());
  const done = (value = null) => Promise.resolve(value);
  switch (command) {
    case 'state': return done(JSON.parse(JSON.stringify(D)));
    case 'count': return done({ total: 95, portrait: 57, landscape: 38 });
    case 'thumbnail': return done(demoGradient(52, 72, 3));
    case 'save_screen': D.screens.find((p) => p.id === args.id).settings = args.settings; return done();
    case 'save_general': D.general = args.general; return done();
    case 'pause': D.paused = args.on; return done();
    case 'new_collection': {
      const c = { id: `c${D.collections.length + 1}`, name: 'New collection', folders: [], recursive: true };
      D.collections.push(c);
      return done(c);
    }
    case 'save_collection': {
      const i = D.collections.findIndex((c) => c.id === args.collection.id);
      D.collections[i] = args.collection;
      return done();
    }
    case 'delete_collection': D.collections = D.collections.filter((c) => c.id !== args.id); return done();
    case 'favorite':
      for (const p of D.screens) for (const f of (p.current ? p.current.sources : [])) if (f.path === args.path) f.favorite = args.on;
      return done();
    default: return done();
  }
}

function demoGradient(w, h, seed) {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  const x = c.getContext('2d');
  const g = x.createLinearGradient(0, 0, w, h);
  g.addColorStop(0, `hsl(${seed * 47 % 360} 55% 45%)`);
  g.addColorStop(1, `hsl(${(seed * 47 + 80) % 360} 60% 25%)`);
  x.fillStyle = g;
  x.fillRect(0, 0, w, h);
  return c.toDataURL();
}

function demoData() {
  const folder = 'C:\\Users\\me\\Pictures\\Wallpapers';
  const settings = (collection, fit) => ({ collection, by_time: false, morning: 'all', afternoon: 'all', night: 'all', every: '30', fit });
  const source = (n, fav) => ({ path: `${folder}\\3\\wallpaper-${n}.jpg`, name: `wallpaper-${n}.jpg`, favorite: fav });
  return {
    screens: [
      { id: 'hp', name: 'HP E202', x: -900, y: -71, w: 900, h: 1600, primary: false, settings: settings('c3', 'auto'), back: true,
        current: { layout: 'fill', sources: [source(7, true)], mini: demoGradient(90, 160, 1) } },
      { id: 'a', name: '2367', x: 0, y: 0, w: 1920, h: 1080, primary: true, settings: settings('all', 'auto'), back: false,
        current: { layout: 'triptych', sources: [source(1, false), source(2, true), source(3, false)], mini: demoGradient(192, 108, 2) } },
      { id: 'b', name: '2367', x: 1920, y: 0, w: 1920, h: 1080, primary: false, settings: settings('c1', 'blur'), back: true,
        current: { layout: 'blur', sources: [source(9, false)], mini: demoGradient(192, 108, 4) } },
    ],
    collections: [
      { id: 'all', name: 'All my wallpapers', folders: [folder], recursive: true },
      { id: 'c1', name: 'Folder 1', folders: [`${folder}\\1`], recursive: true },
      { id: 'c2', name: 'Folder 2', folders: [`${folder}\\2`], recursive: true },
      { id: 'c3', name: 'Folder 3', folders: [`${folder}\\3`], recursive: true },
    ],
    general: { start_with_windows: true, pause_fullscreen: true, accent: true, accent_screen: '', accent_every: 3, morning_from: 6, afternoon_from: 12, night_from: 19,
      language: new URLSearchParams(location.search).get('lang') === 'en' ? 'en' : 'es' },
    hotkeys: [true, true, true, true],
    hidden: [],
    favorites: 2,
    paused: false,
    error: null,
    busy: false,
    focus: null,
    slot: 'night',
    original: true,
    system: new URLSearchParams(location.search).get('os') === 'linux' ? 'linux' : 'windows',
  };
}
