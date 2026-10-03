'use strict';
// Scénarios de l'interface : pilotent le vrai DOM d'app.js (clics, saisies) contre le backend simulé
// de mock-state.js, puis vérifient la configuration éditée et ce qui est envoyé à `save_config`.
// Sortie dans #result : une ligne `ok …` ou `FAIL …` par scénario, puis `DONE <réussis>/<total>`.
// Tout est dans une fonction : les scripts classiques partagent leurs `const` globales avec app.js.

(() => {
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function waitFor(cond, what, ms = 2000) {
  for (let t = 0; t < ms; t += 10) {
    if (cond()) return;
    await sleep(10);
  }
  throw new Error(`délai dépassé : ${what}`);
}
function check(cond, msg) {
  if (!cond) throw new Error(msg);
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const text = (e) => e.textContent.trim();
function button(label, root = document) {
  const b = $$('button', root).find((x) => text(x) === label);
  check(b, `bouton « ${label} » introuvable`);
  return b;
}
function rowOf(label) {
  const r = $$('.row').find((x) => x.querySelector('.row-label') && text(x.querySelector('.row-label')) === label);
  check(r, `ligne « ${label} » introuvable`);
  return r;
}
async function goto(label) {
  $$('.nav-item').find((x) => text(x) === label).click();
  await waitFor(() => $$('.page-title').some((x) => text(x) === label), `page ${label}`);
}
const savebar = () => document.querySelector('.savebar');
async function saveNow() {
  const before = window.__calls.length;
  button('Enregistrer', savebar()).click();
  await waitFor(() => window.__calls.length > before && !savebar(), 'enregistrement');
}

const SCENARIOS = [
  ['chargement : 8 pages, rien à enregistrer', async () => {
    await waitFor(() => $$('.nav-item').length === 8, 'navigation');
    check(!savebar(), 'barre d’enregistrement affichée sans modification');
  }],

  ['textes du système insérés comme texte (pas de HTML)', async () => {
    await goto('Menu contextuel');
    check(document.body.textContent.includes('<b>injection</b>'), 'nom de l’extension absent');
    check(!$$('#app b').some((b) => text(b) === 'injection'), 'balise <b> interprétée');
    check(!$$('#app img').length, 'balise <img> interprétée');
    await sleep(50);
    check(!window.__alerts.length, `script injecté exécuté : alert(${window.__alerts})`);
  }],

  ['volet : masquer Accueil puis enregistrer', async () => {
    await goto('Volet de navigation');
    button('Masquer', rowOf('Accueil')).click();
    check(savebar(), 'pas de barre d’enregistrement après modification');
    await saveNow();
    check(window.__saved.navigation_pane.home === 'hide', `home envoyé = ${window.__saved.navigation_pane.home}`);
    check(window.__saved.navigation_pane.gallery === 'default', 'Galerie modifiée par erreur');
  }],

  ['volet : un nœud est désigné par son nom, « Inchangé » retire la clé', async () => {
    await goto('Volet de navigation');
    button('Masquer', rowOf('Proton Drive')).click();
    check(same(cfg.navigation_pane.nodes, { 'Proton Drive': 'hide' }), JSON.stringify(cfg.navigation_pane.nodes));
    button('Inchangé', rowOf('Proton Drive')).click();
    check(same(cfg.navigation_pane.nodes, {}), JSON.stringify(cfg.navigation_pane.nodes));
    check(!savebar(), 'retour à l’état enregistré non détecté');
  }],

  ['accès rapide : liste blanche nettoyée des guillemets et virgules', async () => {
    await goto('Accès rapide');
    $$('button.choice').find((b) => b.textContent.includes('Liste blanche')).click();
    const t = document.querySelector('textarea');
    check(t, 'zone de saisie absente');
    t.value = '"C:\\Docs",\n\n[\'D:\\X\']';
    t.dispatchEvent(new Event('input'));
    check(same(cfg.quick_access.whitelist, ['C:\\Docs', 'D:\\X']), JSON.stringify(cfg.quick_access.whitelist));
  }],

  ['annuler recharge la configuration enregistrée', async () => {
    button('Annuler', savebar()).click();
    await waitFor(() => !savebar(), 'annulation');
    check(cfg.quick_access.mode === 'default' && same(cfg.quick_access.whitelist, []), JSON.stringify(cfg.quick_access));
  }],

  ['une action est refusée tant que des modifications ne sont pas enregistrées', async () => {
    await goto('Affichage');
    button('Oui', rowOf(VIEW_OPTS[0][1][0][1])).click();
    const before = window.__calls.length;
    await act('apply_now', { profile: null, auto: false, elevate: false, dry_run: true });
    check(window.__calls.length === before, 'commande envoyée malgré des modifications en cours');
    button('Annuler', savebar()).click();
    await waitFor(() => !savebar(), 'annulation');
  }],

  ['profil : une section héritée peut être personnalisée puis rendue', async () => {
    await goto('Affichage');
    const target = document.querySelector('.target-pill select');
    target.value = 'Minimal';
    target.dispatchEvent(new Event('change'));
    await waitFor(() => document.body.textContent.includes('hérite de cette section'), 'bandeau d’héritage');
    button('Personnaliser pour ce profil').click();
    check(same(cfg.profiles.Minimal.explorer_view, cfg.explorer_view), 'copie de la base attendue');
    button('Oui', rowOf(VIEW_OPTS[0][1][0][1])).click();
    check(cfg.explorer_view[VIEW_OPTS[0][1][0][0]] === null, 'la base a été modifiée au lieu du profil');
    button('Revenir à l’héritage').click();
    check(cfg.profiles.Minimal.explorer_view === null, 'section toujours remplacée');
    target.value = '';
    document.querySelector('.target-pill select').value = '';
    document.querySelector('.target-pill select').dispatchEvent(new Event('change'));
  }],

  ['supprimer un profil retire aussi ses règles', async () => {
    window.confirm = () => true;
    await goto('Profils et règles');
    const r = $$('.row').find((x) => x.querySelector('.row-label') && text(x.querySelector('.row-label')).includes('Minimal'));
    check(r, 'ligne du profil introuvable');
    r.querySelector('button.danger').click();
    check(same(cfg.profiles, {}) && same(cfg.rules, []), JSON.stringify({ profiles: cfg.profiles, rules: cfg.rules }));
    await saveNow();
    check(same(window.__saved.rules, []), 'règle orpheline envoyée');
  }],
  ['enregistrer envoie le texte chargé (détection des modifications externes)', async () => {
    await goto('Volet de navigation');
    button('Masquer', rowOf('Galerie')).click();
    await saveNow();
    const call = window.__calls.filter((c) => c[0] === 'save_config').pop();
    check(call && call[1].base === 'version = 1\n', 'base absente : ' + JSON.stringify(call && call[1]));
  }],
];

async function main() {
  const out = document.getElementById('result');
  const lines = [];
  let ok = 0;
  for (const [name, run] of SCENARIOS) {
    try {
      await run();
      ok += 1;
      lines.push(`ok ${name}`);
    } catch (e) {
      lines.push(`FAIL ${name} : ${e.message}`);
    }
  }
  lines.push(`DONE ${ok}/${SCENARIOS.length}`);
  out.textContent = lines.join('\n');
  out.hidden = false;
}

main();
})();
