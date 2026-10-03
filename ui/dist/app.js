'use strict';
// Interface ExplorerBender.
// Sécurité : tout texte venant du système (noms d'extensions, de nœuds, verbes, sorties du CLI) est
// inséré via textContent. Seules les icônes (constantes ci-dessous) passent par innerHTML.

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const clone = (o) => JSON.parse(JSON.stringify(o));
const HOME = '{f874310e-b6b7-47dc-bc84-b9e6b38f5903}';
const GALLERY = '{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}';

let S = null;       // état renvoyé par le backend
let cfg = null;     // configuration en cours d'édition
let saved = '';     // JSON de référence (détection des modifications)
let page = 'overview';
let editing = '';   // '' = configuration de base, sinon nom d'un profil
let lastOutput = '';
let busy = false;
const filters = { ext: '', verbs: '' };

// ---------------------------------------------------------------- icônes (traits 24 px)
const ICONS = {
  logo: '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7A2.5 2.5 0 0 1 18.5 19h-13A2.5 2.5 0 0 1 3 16.5z"/><path d="M8 13h8M12 9.5v7" />',
  overview: '<rect x="3" y="3" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="2"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="13.5" width="7.5" height="7.5" rx="2"/>',
  nav: '<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16M5.5 8h1M5.5 11h1M5.5 14h1"/>',
  quick: '<path d="m12 3 2.6 5.6 6.1.7-4.5 4.2 1.2 6L12 16.6 6.6 19.5l1.2-6-4.5-4.2 6.1-.7z"/>',
  thispc: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  view: '<path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="3"/>',
  context: '<rect x="4" y="3" width="16" height="18" rx="2.5"/><path d="M8 8h8M8 12h8M8 16h5"/>',
  profiles: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0"/><path d="M16 4.6a3.5 3.5 0 0 1 0 6.8M18 14.2a6.5 6.5 0 0 1 3.5 5.8"/>',
  raw: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M13.5 5l-3 14"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  alert: '<path d="M12 3.5 2.5 20h19z"/><path d="M12 10v4.5M12 17.2v.1"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.8v.1"/>',
  play: '<path d="M7 4.5v15l12-7.5z"/>',
  eye: '<path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="3"/>',
  shield: '<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.2 7.5 9.5 4.3-1.3 7.5-4.9 7.5-9.5V6z"/><path d="m9 12 2.2 2.2L15.5 10"/>',
  undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  folder: '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7A2.5 2.5 0 0 1 18.5 19h-13A2.5 2.5 0 0 1 3 16.5z"/>',
  drive: '<rect x="3" y="7" width="18" height="10" rx="2.5"/><path d="M7 12h.01M11 12h6"/>',
  home: '<path d="M4 10.5 12 4l8 6.5V19a1.5 1.5 0 0 1-1.5 1.5H15V15h-6v5.5H5.5A1.5 1.5 0 0 1 4 19z"/>',
  image: '<rect x="3" y="4" width="18" height="16" rx="2.5"/><circle cx="9" cy="9.5" r="1.8"/><path d="m21 16-5-5-9 9"/>',
  cloud: '<path d="M7 18.5a4.5 4.5 0 0 1-.5-9 6 6 0 0 1 11.3 1.6A3.8 3.8 0 0 1 17.5 18.5z"/>',
  node: '<circle cx="12" cy="12" r="3.2"/><path d="M12 3v5.8M12 15.2V21M3 12h5.8M15.2 12H21"/>',
  pin: '<path d="M9 4h6l-1 6 3.5 3.5h-11L10 10z"/><path d="M12 13.5V21"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>',
  ban: '<circle cx="12" cy="12" r="9"/><path d="m5.6 5.6 12.8 12.8"/>',
  list: '<path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M5.5 7l1 12.5A1.5 1.5 0 0 0 8 21h8a1.5 1.5 0 0 0 1.5-1.5l1-12.5M9 7V4.5h6V7"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>',
  bolt: '<path d="M13 2.5 4.5 13.5H11L10 21.5l8.5-11H12z"/>',
  windows: '<path d="M3.5 5.5 10.5 4.5v7h-7zM12.5 4.2l8-1.2v8.5h-8zM3.5 13.5h7v7l-7-1zM12.5 13.5h8V21l-8-1.2z"/>',
  layers: '<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>',
  sparkle: '<path d="M12 3.5 13.8 10.2 20.5 12l-6.7 1.8L12 20.5l-1.8-6.7L3.5 12l6.7-1.8z"/>',
};

function icon(name) {
  const s = document.createElement('span');
  s.style.display = 'contents';
  s.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name] || ''}</svg>`;
  return s.firstElementChild;
}

const TONES = {
  overview: ['#8b5cf6', '#3b82f6'],
  nav: ['#8b5cf6', '#6366f1'],
  quick: ['#f59e0b', '#f43f5e'],
  thispc: ['#06b6d4', '#3b82f6'],
  view: ['#10b981', '#0ea5e9'],
  context: ['#ec4899', '#8b5cf6'],
  profiles: ['#6366f1', '#06b6d4'],
  raw: ['#64748b', '#334155'],
};
const grad = (t) => `linear-gradient(135deg, ${TONES[t][0]}, ${TONES[t][1]})`;

// ---------------------------------------------------------------- DOM

function el(tag, props = {}, ...kids) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') e.className = v;
    else if (k === 'style') e.setAttribute('style', v);
    else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
    else if (k === 'text') e.textContent = v;
    else if (k in e) e[k] = v;
    else e.setAttribute(k, v);
  }
  for (const kid of kids.flat()) if (kid !== null && kid !== undefined && kid !== false) e.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  return e;
}

const isDirty = () => cfg !== null && JSON.stringify(cfg) !== saved;
const lc = (s) => String(s).trim().toLowerCase();
// Tolère les formes collées depuis un TOML ("C:\Docs", 'C:\Docs', ["C:\Docs") : guillemets, virgules, crochets retirés.
const cleanLine = (x) => x.replace(/^[\s[\]"',]+|[\s[\]"',]+$/g, '');

let toastEl = null;
function toast(msg, kind = 'ok') {
  if (!toastEl) { toastEl = el('div', { class: 'toast', role: 'status', 'aria-live': 'polite' }); document.body.append(toastEl); }
  toastEl.className = `toast ${kind}`;
  toastEl.replaceChildren(icon(kind === 'bad' ? 'alert' : 'check'), el('span', { text: msg }));
  requestAnimationFrame(() => toastEl.classList.add('show'));
  clearTimeout(toast.t);
  toast.t = setTimeout(() => toastEl.classList.remove('show'), 3800);
}

// ---------------------------------------------------------------- composants

function seg(options, get, set) {
  const box = el('div', { class: 'seg', role: 'group' });
  const draw = () => box.replaceChildren(...options.map(([value, label, tone]) => el('button', {
    type: 'button', text: label, class: tone || 'neutral', 'aria-pressed': String(get() === value),
    onclick: () => { set(value); changed(); draw(); },
  })));
  draw();
  return box;
}

/** Trois états : non renseigné (null) / Oui / Non. */
function tri(o, key, unsetLabel = 'Inchangé') {
  return seg([['', unsetLabel], ['true', 'Oui', 'on'], ['false', 'Non', 'off']],
    () => (o[key] === null || o[key] === undefined ? '' : String(o[key])),
    (v) => { o[key] = v === '' ? null : v === 'true'; });
}

function vis(get, set) {
  return seg([['default', 'Inchangé'], ['show', 'Afficher', 'on'], ['hide', 'Masquer', 'off']], () => get() || 'default', set);
}

function toggle(checked, onchange) {
  const input = el('input', { type: 'checkbox', checked });
  input.addEventListener('change', () => { onchange(input.checked); changed(); });
  return el('label', { class: 'switch' }, input, el('span', { class: 'track' }));
}

function row(label, control, hint, ic) {
  return el('div', { class: 'row' },
    ic ? el('div', { class: 'row-icon' }, icon(ic)) : null,
    el('div', { class: 'row-text' }, typeof label === 'string' ? el('div', { class: 'row-label', text: label }) : el('div', { class: 'row-label' }, label), hint ? el('div', { class: 'row-hint', text: hint }) : null),
    control);
}

function card(title, sub, ...children) {
  return el('section', { class: 'card' },
    title ? el('div', { class: 'card-head' }, typeof title === 'string' ? el('div', { class: 'card-title', text: title }) : title) : null,
    sub ? el('div', { class: 'card-sub', text: sub }) : null,
    ...children);
}

function pageHead(id, title, lead) {
  return el('div', { class: 'page-head' },
    el('div', { class: 'page-icon', style: `background:${grad(id)};--tone:${TONES[id][0]}` }, icon(id)),
    el('div', {}, el('h1', { class: 'page-title', text: title }), el('p', { class: 'page-lead', text: lead })));
}

function btn(label, ic, onclick, cls = '', disabled = false) {
  return el('button', { type: 'button', class: `btn ${cls}`, disabled, onclick }, ic ? icon(ic) : null, label);
}

function lines(o, key, rows = 6) {
  const t = el('textarea', { rows, spellcheck: false, placeholder: 'C:\\Documents\nC:\\Desktop' });
  t.value = (o[key] || []).join('\n');
  t.addEventListener('input', () => { o[key] = t.value.split('\n').map(cleanLine).filter(Boolean); changed(); });
  return t;
}

function search(key, placeholder, onInput) {
  const i = el('input', { type: 'text', placeholder, value: filters[key] });
  i.addEventListener('input', () => { filters[key] = i.value; onInput(); });
  return el('div', { class: 'search' }, icon('search'), i);
}

function inList(arr, ...names) { return arr.some((x) => names.map(lc).includes(lc(x))); }
function setInList(arr, on, value, ...aliases) {
  const drop = [value, ...aliases].map(lc);
  const out = arr.filter((x) => !drop.includes(lc(x)));
  if (on) out.push(value);
  arr.length = 0;
  arr.push(...out);
}

/** Section éditée : la base, ou la section remplacée d'un profil (null = héritée). */
function secObj(name) {
  return editing ? cfg.profiles[editing][name] : cfg[name];
}

/** Bandeau « profil » : héritage ou remplacement de la section. */
function profileGate(name) {
  if (!editing) return null;
  const p = cfg.profiles[editing];
  if (!p[name]) {
    return card(null, null, row(`Le profil « ${editing} » hérite de cette section`, btn('Personnaliser pour ce profil', 'edit', () => { p[name] = clone(cfg[name]); changed(); render(); }, 'primary sm'), 'Elle n’est pas remplacée : la configuration de base s’applique.', 'layers'));
  }
  return card(null, null, row(`Section remplacée pour le profil « ${editing} »`, btn('Revenir à l’héritage', 'undo', () => { p[name] = null; changed(); render(); }, 'sm'), null, 'layers'));
}

// ---------------------------------------------------------------- pages

function pageOverview() {
  const st = S.status || {};
  const conflicts = st.conflicts || [];
  const running = S.daemon_running && !S.suspended;
  const out = [];
  out.push(el('div', { class: 'hero' },
    el('div', { class: 'hero-top' }, el('span', { class: `dot ${running ? 'ok' : 'warn'}` }), running ? 'Le démon veille en arrière-plan' : S.suspended ? 'Démon suspendu par « Tout restaurer »' : 'Démon arrêté'),
    el('div', { class: 'hero-title', text: running ? 'Votre Explorateur est sous contrôle' : 'ExplorerBender est en pause' }),
    el('p', { class: 'hero-text', text: 'Déclarez ce que vous voulez voir dans l’Explorateur ; chaque enregistrement est appliqué tout de suite et maintenu si Windows tente de le défaire.' }),
    el('div', { class: 'hero-actions' },
      btn(busy ? 'En cours…' : 'Appliquer maintenant', busy ? null : 'play', () => act('apply_now', { profile: null, auto: false, elevate: false, dryRun: false }), 'solid', busy),
      btn('Aperçu', 'eye', () => act('apply_now', { profile: null, auto: false, elevate: false, dryRun: true }), '', busy),
      btn('Réglages protégés (UAC)', 'shield', () => act('apply_now', { profile: null, auto: false, elevate: true, dryRun: false }), '', busy))));

  const tile = (tone, ic, label, value, hint) => el('div', { class: 'tile' },
    el('div', { class: 'tile-top' }, el('div', { class: 'tile-icon', style: `background:${grad(tone)}` }, icon(ic)), label),
    el('div', { class: 'tile-value', text: value }), hint ? el('div', { class: 'tile-hint', text: hint }) : null);
  out.push(el('div', { class: 'tiles' },
    tile('thispc', 'windows', 'Windows', `Build ${S.windows_build}`, 'Tweaks validés pour 24H2/25H2'),
    tile('profiles', 'profiles', 'Profil actif', S.profile || 'Base', S.profile ? (S.profile_manual ? 'choisi manuellement' : 'choisi par une règle') : 'aucun profil appliqué'),
    tile(conflicts.length ? 'quick' : 'view', conflicts.length ? 'alert' : 'check', 'Conflits', conflicts.length ? `${conflicts.length}` : 'Aucun', conflicts.length ? 'un autre outil réécrit des valeurs' : 'aucun outil ne se bat avec nous')));

  if (conflicts.length) out.push(card('Valeurs réécrites par un autre outil', 'Le démon a cessé de les corriger pour ne pas entrer en boucle. Modifiez la configuration ou redémarrez l’Explorateur pour réessayer.', ...conflicts.map((c) => row(el('span', { class: 'mono', text: c }), null, null, 'alert'))));

  const changes = (st.last_changes || []).filter((c) => c.kind !== 'Unchanged');
  const KIND = { Applied: ['Appliqué', 'ok'], Reverted: ['Restauré', 'accent'], Skipped: ['Ignoré', 'warn'], Conflict: ['Conflit', 'bad'], Error: ['Erreur', 'bad'], WouldApply: ['À appliquer', ''], WouldRevert: ['À restaurer', ''] };
  out.push(card(el('div', { class: 'card-title' }, 'Dernière activité du démon ', changes.length ? el('span', { class: 'count', text: String(changes.length) }) : null),
    st.last_apply_at ? `Dernière passe : ${new Date(st.last_apply_at).toLocaleString('fr-FR')}` : null,
    ...(changes.length ? changes.slice(0, 10).map((c) => {
      const [label, tone] = KIND[c.kind] || [c.kind, ''];
      return row(c.what, el('span', { class: `badge ${tone}`, text: label }), null, c.kind === 'Error' || c.kind === 'Conflict' ? 'alert' : 'check');
    }) : [el('div', { class: 'empty', text: 'Rien à signaler : tout est conforme.' })])));

  if (lastOutput) out.push(card('Résultat de la dernière action', null, el('pre', { class: 'out', text: lastOutput })));

  out.push(card('Zone de retour arrière', 'Remet exactement l’état d’origine de l’Explorateur (épingles, nœuds, valeurs de registre) et suspend le démon. « Appliquer maintenant » le réactive.',
    el('div', { class: 'actions' }, btn('Tout restaurer', 'undo', async () => {
      if (confirm('Remettre exactement l’état d’origine de l’Explorateur et suspendre le démon ?')) await act('restore_now', {});
    }, 'danger', busy)),
    row(el('span', { class: 'mono', text: S.config_path }), null, 'Fichier de configuration', 'raw')));
  return out;
}

const NODE_ICON = (name) => (/drive|cloud|onedrive|mega|dropbox/i.test(name) ? 'cloud' : /biblio|librar/i.test(name) ? 'layers' : /rapide|quick/i.test(name) ? 'quick' : 'node');

function pageNav() {
  const out = [pageHead('nav', 'Volet de navigation', 'Choisissez les nœuds affichés à gauche de l’Explorateur. Seule une valeur de votre profil utilisateur est écrite ; la définition système n’est jamais modifiée.')];
  const gate = profileGate('navigation_pane');
  if (gate) out.push(gate);
  const o = secObj('navigation_pane');
  if (!o) return out;
  out.push(card('Essentiels', null,
    row('Accueil', vis(() => o.home, (v) => { o.home = v; }), 'Page d’accueil avec fichiers récents et recommandations', 'home'),
    row('Galerie', vis(() => o.gallery, (v) => { o.gallery = v; }), 'Vue chronologique de vos photos', 'image')));
  const names = S.nodes.map((n) => n.name);
  const rows = S.nodes.filter((n) => lc(n.clsid) !== HOME && lc(n.clsid) !== GALLERY).map((n) => {
    const key = names.filter((x) => x === n.name).length === 1 ? n.name : n.clsid;
    const cur = () => Object.keys(o.nodes).find((k) => lc(k) === lc(key) || lc(k) === lc(n.clsid));
    const eff = n.effective === null ? 'état inconnu' : n.effective === 0 ? 'actuellement masqué' : 'actuellement affiché';
    return row(n.name, vis(() => { const c = cur(); return c ? o.nodes[c] : 'default'; }, (v) => {
      const c = cur();
      if (c) delete o.nodes[c];
      if (v !== 'default') o.nodes[key] = v;
    }), eff, NODE_ICON(n.name));
  });
  out.push(card(el('div', { class: 'card-title' }, 'Autres nœuds détectés ', el('span', { class: 'count', text: String(rows.length) })), 'Réseau, Linux et OneDrive n’apparaissent que s’ils sont pilotables ainsi sur votre version de Windows.', ...rows));
  return out;
}

function pageQuick() {
  const out = [pageHead('quick', 'Accès rapide', 'Videz l’Accès rapide et gardez-le vide, ou n’y laissez que les dossiers que vous choisissez. Tout ré-épinglage par Windows ou une application est annulé.')];
  const gate = profileGate('quick_access');
  if (gate) out.push(gate);
  const o = secObj('quick_access');
  if (!o) return out;
  const MODES = [
    ['default', 'Inchangé', 'Windows gère l’Accès rapide comme d’habitude.', 'undo', 'raw'],
    ['disabled', 'Désactivé', 'Aucune épingle, aucun dossier fréquent, aucun fichier récent.', 'ban', 'quick'],
    ['whitelist', 'Liste blanche', 'Seuls vos dossiers choisis restent épinglés.', 'list', 'nav'],
  ];
  out.push(card('Mode', null, el('div', { class: 'choice-grid' }, ...MODES.map(([v, t, d, ic, tone]) => el('button', {
    type: 'button', class: 'choice', 'aria-pressed': String(o.mode === v), onclick: () => { o.mode = v; changed(); render(); },
  }, el('div', { class: 'c-check' }, o.mode === v ? icon('check') : null), el('div', { class: 'c-icon', style: `background:${grad(tone)}` }, icon(ic)), el('div', { class: 'c-title', text: t }), el('div', { class: 'c-text', text: d }))))));
  if (o.mode === 'whitelist') out.push(card('Dossiers à garder épinglés', 'Un chemin par ligne. Les guillemets et virgules collés depuis un fichier TOML sont ignorés.', el('div', { class: 'pad' }, lines(o, 'whitelist'))));
  if (o.mode !== 'disabled') {
    // En liste blanche, « non renseigné » = masqué (le cœur force false).
    const unset = o.mode === 'whitelist' ? 'Masqués' : 'Inchangé';
    out.push(card('Contenu automatique', o.mode === 'whitelist' ? 'Masqués par défaut en liste blanche, pour que rien ne réapparaisse à côté de vos épingles.' : null,
      row('Dossiers fréquents', tri(o, 'show_frequent', unset), 'Dossiers que vous ouvrez souvent', 'clock'),
      row('Fichiers récents', tri(o, 'show_recent', unset), 'Derniers fichiers ouverts', 'list')));
  }
  return out;
}

function folderState(o, f) {
  if (inList(o.hide_folders, f.id, f.label)) return 'hide';
  if (inList(o.show_folders, f.id, f.label)) return 'show';
  return 'default';
}

function pageThisPc() {
  const out = [pageHead('thispc', 'Ce PC', 'Dossiers et lecteurs visibles dans « Ce PC ». Windows protège ces réglages : ils s’appliquent avec le bouton « Réglages protégés (UAC) », jamais en arrière-plan.')];
  const gate = profileGate('this_pc');
  if (gate) out.push(gate);
  const o = secObj('this_pc');
  if (!o) return out;
  out.push(el('div', { class: 'banner' }, icon('shield'), el('div', { text: 'Ces options demandent une confirmation administrateur (invite UAC) au moment de les appliquer.' })));
  out.push(card('Dossiers', null, ...S.folders.map((f) => row(f.label, vis(() => folderState(o, f), (v) => {
    setInList(o.hide_folders, false, f.id, f.label);
    setInList(o.show_folders, false, f.id, f.label);
    if (v === 'hide') o.hide_folders.push(f.id);
    if (v === 'show') o.show_folders.push(f.id);
  }), null, 'folder'))));
  out.push(card('Lecteurs à masquer', 'Masque le lecteur dans l’Explorateur, sans empêcher d’y accéder en tapant son chemin (ex. D:\\).',
    ...S.drives.map((d) => row(`${d.letter}:  ${d.label || 'Disque local'}`, toggle(inList(o.hide_drives, d.letter), (on) => setInList(o.hide_drives, on, d.letter)), `Lecteur ${d.kind}`, 'drive'))));
  return out;
}

const VIEW_OPTS = [
  ['Fichiers', [
    ['show_file_extensions', 'Extensions de fichiers', 'Afficher « .txt », « .pdf »…'],
    ['show_hidden_files', 'Fichiers et dossiers cachés', null],
    ['show_system_files', 'Fichiers protégés du système', 'À éviter sauf besoin précis'],
    ['use_checkboxes', 'Cases à cocher de sélection', null],
  ]],
  ['Volet de navigation', [
    ['nav_show_all_folders', 'Afficher tous les dossiers', null],
    ['nav_expand_to_current_folder', 'Développer jusqu’au dossier courant', null],
  ]],
  ['Fenêtre', [
    ['compact_mode', 'Mode compact', 'Espacement réduit entre les éléments'],
    ['show_status_bar', 'Barre d’état', null],
    ['hide_drives_with_no_media', 'Masquer les lecteurs vides', 'Lecteurs de cartes, DVD sans disque…'],
    ['sync_provider_notifications', 'Suggestions des fournisseurs de synchronisation', 'Publicités OneDrive et autres dans l’Explorateur'],
  ]],
];

function pageView() {
  const out = [pageHead('view', 'Affichage', 'Options d’affichage de l’Explorateur. « Inchangé » laisse le choix de Windows. Les fenêtres déjà ouvertes se mettent à jour à leur réouverture.')];
  const gate = profileGate('explorer_view');
  if (gate) out.push(gate);
  const o = secObj('explorer_view');
  if (!o) return out;
  out.push(card('Ouverture', null, row('Ouvrir l’Explorateur sur', seg([['', 'Inchangé'], ['this_pc', 'Ce PC'], ['home', 'Accueil'], ['downloads', 'Téléchargements'], ['onedrive', 'Cloud']], () => o.launch_to || '', (v) => { o.launch_to = v || null; }), null, 'home')));
  for (const [title, opts] of VIEW_OPTS) out.push(card(title, null, ...opts.map(([k, label, hint]) => row(label, tri(o, k), hint))));
  return out;
}

function pageContext() {
  const out = [pageHead('context', 'Menu contextuel', 'Allégez le clic droit. Méthodes non destructives : rien de ce que les applications ont installé n’est supprimé.')];
  const gate = profileGate('context_menu');
  if (gate) out.push(gate);
  const o = secObj('context_menu');
  if (!o) return out;
  out.push(card(null, null, row('Menu contextuel classique', toggle(o.classic_menu, (on) => { o.classic_menu = on; }), 'Le menu complet de Windows 10, sans « Afficher plus d’options ». Visible dans les nouvelles fenêtres.', 'sparkle')));

  const extBox = el('div');
  const verbBox = el('div');
  const drawExt = () => {
    const list = S.extensions.filter((e) => lc(e.name + e.clsid).includes(lc(filters.ext)));
    extBox.replaceChildren(...(list.length ? list.slice(0, 300).map((e) => row(e.name,
      toggle(inList(o.blocked_extensions, e.clsid, e.name), (on) => setInList(o.blocked_extensions, on, e.clsid, e.name)),
      e.blocked && !inList(o.blocked_extensions, e.clsid, e.name) ? 'déjà bloquée par un autre réglage' : e.clsid)) : [el('div', { class: 'empty', text: 'Aucune extension ne correspond.' })]));
  };
  const drawVerbs = () => {
    const list = S.verbs.filter((v) => lc(v).includes(lc(filters.verbs)));
    verbBox.replaceChildren(...(list.length ? list.slice(0, 300).map((v) => row(el('span', { class: 'mono', text: v }),
      toggle(inList(o.disabled_verbs, v), (on) => setInList(o.disabled_verbs, on, v)))) : [el('div', { class: 'empty', text: 'Aucun verbe ne correspond.' })]));
  };
  drawExt();
  drawVerbs();
  out.push(card(el('div', { class: 'card-title' }, 'Extensions à bloquer ', el('span', { class: 'count', text: String(S.extensions.length) })), 'Activez l’interrupteur pour retirer l’extension du menu.', search('ext', 'Rechercher une extension…', drawExt), extBox));
  out.push(card(el('div', { class: 'card-title' }, 'Commandes à désactiver ', el('span', { class: 'count', text: String(S.verbs.length) })), 'Entrées simples ajoutées par les applications (« Ouvrir avec Code », « Git Bash »…).', search('verbs', 'Rechercher une commande…', drawVerbs), verbBox));
  return out;
}

function pageProfiles() {
  const out = [pageHead('profiles', 'Profils et règles', 'Un profil remplace certaines sections de la configuration. Les règles choisissent le profil automatiquement ; un choix manuel les supplante jusqu’au retour au mode automatique.')];
  const names = Object.keys(cfg.profiles);
  const items = names.map((n) => {
    const sections = Object.entries(cfg.profiles[n]).filter(([, v]) => v).map(([k]) => ({ navigation_pane: 'volet', quick_access: 'accès rapide', this_pc: 'Ce PC', explorer_view: 'affichage', context_menu: 'menu' }[k])).join(', ') || 'hérite de tout';
    return row(el('span', {}, n, ' ', S.profile === n ? el('span', { class: 'badge ok', text: 'actif' }) : null),
      el('div', { style: 'display:flex;gap:6px' },
        btn('Modifier', 'edit', () => { editing = n; page = 'nav'; render(); }, 'sm'),
        btn('Activer', 'play', () => act('apply_now', { profile: n, auto: false, elevate: false, dryRun: false }), 'sm primary', busy || isDirty()),
        btn('', 'trash', () => { if (confirm(`Supprimer le profil « ${n} » ?`)) { delete cfg.profiles[n]; cfg.rules = cfg.rules.filter((r) => r.profile !== n); if (editing === n) editing = ''; changed(); render(); } }, 'sm ghost danger')),
      `Remplace : ${sections}`, 'profiles');
  });
  const nameIn = el('input', { type: 'text', placeholder: 'Nom du nouveau profil (ex. Travail)', style: 'flex:1' });
  const create = () => {
    const n = nameIn.value.trim();
    if (!n || /[.[\]"\\]/.test(n)) return toast('Nom invalide (évitez . [ ] " \\)', 'bad');
    if (cfg.profiles[n]) return toast('Ce profil existe déjà', 'bad');
    cfg.profiles[n] = { navigation_pane: clone(cfg.navigation_pane), quick_access: clone(cfg.quick_access), this_pc: clone(cfg.this_pc), explorer_view: clone(cfg.explorer_view), context_menu: clone(cfg.context_menu) };
    changed(); render();
  };
  nameIn.addEventListener('keydown', (e) => { if (e.key === 'Enter') create(); });
  out.push(card(el('div', { class: 'card-title' }, 'Profils ', el('span', { class: 'count', text: String(names.length) })), null,
    ...(items.length ? items : [el('div', { class: 'empty', text: 'Aucun profil pour l’instant.' })]),
    el('div', { class: 'row' }, nameIn, btn('Créer', 'plus', create, 'primary'))));

  const rules = cfg.rules.map((r, i) => {
    const kind = r.when.drive_absent ? 'absent' : 'present';
    const letter = el('input', { type: 'text', size: 2, maxLength: 1, value: r.when.drive_absent || r.when.drive_present || '', style: 'width:46px;text-align:center;font-weight:600' });
    const prof = el('select', {}, ...names.map((n) => el('option', { value: n, text: n })));
    prof.value = r.profile;
    let k = kind;
    const sync = () => {
      const l = letter.value.trim().toUpperCase().slice(0, 1) || null;
      r.when = k === 'absent' ? { drive_present: null, drive_absent: l } : { drive_present: l, drive_absent: null };
      r.profile = prof.value;
      changed();
    };
    letter.addEventListener('input', sync);
    prof.addEventListener('change', sync);
    return el('div', { class: 'row' },
      el('div', { class: 'row-icon' }, icon('bolt')),
      el('span', { class: 'row-label', text: `Si le lecteur` }), letter,
      seg([['present', 'est présent'], ['absent', 'est absent']], () => k, (v) => { k = v; sync(); }),
      el('span', { class: 'row-label', text: 'alors' }), prof,
      el('div', { class: 'spacer' }),
      btn('', 'trash', () => { cfg.rules.splice(i, 1); changed(); render(); }, 'sm ghost danger'));
  });
  out.push(card(el('div', { class: 'card-title' }, 'Règles automatiques ', el('span', { class: 'count', text: String(rules.length) })), 'Évaluées dans l’ordre ; la première qui correspond gagne. Réévaluées quand un lecteur est branché ou retiré.',
    ...(rules.length ? rules : [el('div', { class: 'empty', text: names.length ? 'Aucune règle.' : 'Créez d’abord un profil.' })]),
    el('div', { class: 'actions' },
      btn('Ajouter une règle', 'plus', () => { cfg.rules.push({ when: { drive_present: null, drive_absent: 'D' }, profile: names[0] }); changed(); render(); }, '', !names.length),
      btn('Revenir au mode automatique', 'bolt', () => act('apply_now', { profile: null, auto: true, elevate: false, dryRun: false }), '', busy || isDirty()))));
  return out;
}

function pageRaw() {
  const t = el('textarea', { class: 'big', spellcheck: false });
  t.value = S.config_raw || '# config.toml absent : « Enregistrer le TOML » le créera\n';
  return [pageHead('raw', 'TOML avancé', 'Édition directe de config.toml. Les autres pages modifient aussi le fichier en place, commentaires conservés (l’ancienne version reste dans config.toml.bak).'),
    card(null, null, el('div', { class: 'pad', style: 'padding-top:18px' }, t), el('div', { class: 'actions' }, btn('Enregistrer le TOML', 'check', async () => {
      try { await invoke('save_raw', { text: t.value }); toast('config.toml enregistré'); await load(); } catch (e) { toast(String(e), 'bad'); }
    }, 'primary')))];
}

const PAGES = [
  ['overview', 'Vue d’ensemble', pageOverview, 'Général'],
  ['nav', 'Volet de navigation', pageNav, 'Explorateur'],
  ['quick', 'Accès rapide', pageQuick],
  ['thispc', 'Ce PC', pageThisPc],
  ['view', 'Affichage', pageView],
  ['context', 'Menu contextuel', pageContext],
  ['profiles', 'Profils et règles', pageProfiles, 'Automatisation'],
  ['raw', 'TOML avancé', pageRaw],
];

// ---------------------------------------------------------------- actions et rendu

async function load() {
  S = await invoke('get_state');
  cfg = clone(S.config);
  // Valeurs déjà enregistrées avec des guillemets littéraux : affichées nettoyées (le cœur les tolère aussi).
  cfg.quick_access.whitelist = cfg.quick_access.whitelist.map(cleanLine).filter(Boolean);
  saved = JSON.stringify(cfg);
  if (editing && !cfg.profiles[editing]) editing = '';
  render();
}

async function act(cmd, args) {
  if (isDirty()) return toast('Enregistrez d’abord vos modifications.', 'bad');
  busy = true; render();
  try {
    const r = await invoke(cmd, args);
    lastOutput = r.output.trim() || '(aucune sortie)';
    toast(r.code === 0 ? 'Terminé' : `Terminé avec des erreurs (code ${r.code})`, r.code === 0 ? 'ok' : 'bad');
  } catch (e) {
    lastOutput = String(e);
    toast('L’action a échoué', 'bad');
  }
  busy = false;
  page = 'overview';
  await load();
}

async function save() {
  try { await invoke('save_config', { config: cfg }); toast('Enregistré — appliqué par le démon'); await load(); } catch (e) { toast(String(e), 'bad'); }
}

let savebar = null;
function changed() {
  const d = isDirty();
  if (d && !savebar) {
    savebar = el('div', { class: 'savebar', role: 'region', 'aria-label': 'Modifications non enregistrées' },
      el('span', { class: 'dot' }), el('span', { text: 'Modifications non enregistrées' }),
      btn('Annuler', null, () => load(), 'ghost sm'), btn('Enregistrer', 'check', save, 'primary sm'));
    document.body.append(savebar);
  } else if (!d && savebar) {
    savebar.remove();
    savebar = null;
  }
}

function render() {
  const app = document.getElementById('app');
  const scroller = app.querySelector('.scroll');
  const keepScroll = scroller && render.lastPage === page ? scroller.scrollTop : 0;
  render.lastPage = page;

  const nav = el('nav', { class: 'nav' });
  for (const [id, label, , group] of PAGES) {
    if (group) nav.append(el('div', { class: 'nav-label', text: group }));
    nav.append(el('button', { type: 'button', class: 'nav-item', 'aria-current': id === page ? 'page' : null, onclick: () => { page = id; render(); } }, icon(id), label));
  }
  const running = S.daemon_running && !S.suspended;
  const side = el('aside', { class: 'side' },
    el('div', { class: 'brand' }, el('div', { class: 'brand-mark' }, icon('logo')), el('div', {}, el('div', { class: 'brand-name', text: 'ExplorerBender' }), el('div', { class: 'brand-sub', text: 'Structure de l’Explorateur' }))),
    nav,
    el('div', { class: 'side-foot' }, el('div', { class: 'daemon-chip' }, el('span', { class: `dot ${running ? 'ok' : 'warn'}` }),
      el('div', {}, el('div', { style: 'font-weight:600;color:var(--ink)', text: running ? 'Démon actif' : S.suspended ? 'Démon suspendu' : 'Démon arrêté' }), el('div', { text: `Windows build ${S.windows_build}` })))));

  const target = el('select', {}, el('option', { value: '', text: 'Configuration de base' }), ...Object.keys(cfg.profiles).map((n) => el('option', { value: n, text: `Profil « ${n} »` })));
  target.value = editing;
  target.addEventListener('change', () => { editing = target.value; render(); });
  const current = PAGES.find(([id]) => id === page) || PAGES[0];
  const topbar = el('header', { class: 'topbar' },
    el('div', { class: 'crumb', text: `ExplorerBender  ›  ${current[1]}` }),
    el('div', { class: 'spacer' }),
    page !== 'overview' && page !== 'profiles' && page !== 'raw' ? el('label', { class: 'target-pill' }, 'Édition de', target) : null);

  const pageEl = el('div', { class: 'page' });
  if (S.config_error) pageEl.append(el('div', { class: 'banner bad' }, icon('alert'), el('div', { text: `config.toml invalide, l’ancienne configuration reste active : ${S.config_error}` })));
  else if (S.profile_warning) pageEl.append(el('div', { class: 'banner' }, icon('info'), el('div', { text: S.profile_warning })));
  pageEl.append(...current[2]());

  const scroll = el('div', { class: 'scroll' }, pageEl);
  app.replaceChildren(side, el('main', {}, topbar, scroll));
  scroll.scrollTop = keepScroll;
  changed();
}

document.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') { e.preventDefault(); if (isDirty()) save(); }
});

const app = document.getElementById('app');
if (window.__TAURI__) {
  app.replaceChildren(el('div', { class: 'loading' }, el('div', { class: 'spin' }), 'Lecture de la configuration…'));
  load().catch((e) => { app.replaceChildren(el('div', { class: 'loading', text: `Erreur de chargement : ${e}` })); });
} else {
  app.replaceChildren(el('div', { class: 'loading', text: 'Cette page doit être ouverte dans l’application ExplorerBender.' }));
}
