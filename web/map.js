'use strict';
/* Offline map drawn from local vector tiles (/api/map/tile/z/x/y) with clustered help + resource pins (/api/map/pins).
   No external tiles or libraries. */
const MapView = (() => {
  const EXT = 512;            // tile coordinate extent sent by the server
  const MINZ = 3, MAXZ = 19, DATAZ = 17;
  let HOME = null;   // a household's own opening place, from web/home-area.json: a local file never packaged. "Set as my area" wins over it.
  const IN_WINDOW = location.protocol === 'lastbook:' || location.hostname === 'lastbook.invalid';
  const EUROPE = [-25, 34, 45, 71.5];  // the EU and EFTA, from the Azores to the North Cape: the whole map when the server names no bounds
  const FULL = ['<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/></svg>', '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>'];   // full screen, and leave it
  const C = { land: '#f2efe6', ocean: '#aad3df', water: '#aad3df', wet: '#cfe3d4', beach: '#f4e7b6',
    park: '#cfe7c2', playa: '#ece4cf', green: '#d5e8c6', farm: '#eceac6', res: '#ebe7de', com: '#f1e2df', ind: '#e8e1ea', bldg: '#d9d0c3', bldgLine: '#c6bcae',
    waterway: '#8cc0d6', rail: '#8f8f8f', label: '#2a2f2c', halo: 'rgba(255,255,255,.9)' };
  const ROAD = { motorway: ['#e9a03b', 5, 5], trunk: ['#efb556', 4.5, 6], primary: ['#f6d57c', 4, 8], secondary: ['#fdf3c4', 3.5, 10],
    tertiary: ['#ffffff', 3, 11], residential: ['#ffffff', 2.2, 13], unclassified: ['#ffffff', 2.2, 13], living_street: ['#ffffff', 2, 14],
    service: ['#ffffff', 1.4, 15], pedestrian: ['#efeae4', 2, 14], track: ['#c8a777', 1.2, 13], footway: ['#d08f7a', 1, 15], path: ['#d08f7a', 1, 14],
    cycleway: ['#6f8fd8', 1, 14], steps: ['#d08f7a', 1.6, 16], bridleway: ['#7fae6a', 1, 14], busway: ['#ffffff', 2, 14] };
  const roadStyle = c => ROAD[c] || ROAD[c.replace(/_link$/, '')] || (c.startsWith('track') ? ROAD.track : ROAD.service);

  // Map layers: id, name (a key of web/locales/), symbol, colour, server classes, group
  const LAYERS = [
    ['hospital', 'map.layer.hospital', 'H', '#c62828', 'hospital', 'help'],
    ['clinic', 'map.layer.clinic', '+', '#d81b60', 'clinic,doctors', 'help'],
    ['dentist', 'map.layer.dentist', 'Dt', '#00695c', 'dentist', 'help'],
    ['pharmacy', 'map.layer.pharmacy', '✚', '#2e7d32', 'pharmacy', 'help'],   // the green cross of Europe's pharmacies: amenity=pharmacy alone
    ['fire', 'map.layer.fire', 'F', '#ef6c00', 'fire_station', 'help'],
    ['police', 'map.layer.police', 'P', '#1a4f9c', 'police', 'help'],
    ['shelter', 'map.layer.shelter', '⌂', '#455a64', 'shelter', 'help'],
    ['community', 'map.layer.community', 'C', '#8e24aa', 'community_centre,town_hall,library', 'help'],
    ['water', 'map.layer.water', 'W', '#0277bd', 'drinking_water', 'water'],
    ['reservoir', 'map.layer.reservoir', '≈', '#1565c0', 'reservoir,lake', 'water'],
    ['pond', 'map.layer.pond', '≈', '#5e9fd0', 'pond', 'water'],
    ['dam', 'map.layer.dam', 'D', '#6d4c41', 'dam', 'water'],
    ['spring', 'map.layer.spring', 'S', '#00897b', 'spring', 'water'],
    ['food', 'map.layer.food', '$', '#a05f00', 'supermarket,convenience', 'supply'],
    ['hardware', 'map.layer.hardware', '⚒', '#546e7a', 'hardware,doityourself', 'supply'],
    ['drugstore', 'map.layer.drugstore', 'Dr', '#8e5572', 'chemist', 'supply'],   // shop=chemist: dm, Rossmann, Müller. No prescription medicines, so not under the cross
    ['fuel', 'map.layer.fuel', 'G', '#37474f', 'fuel', 'supply'],
    ['camp', 'map.layer.camp', '▲', '#558b2f', 'camp_site', 'supply'],
    ['orchard', 'map.layer.orchard', '♣', '#3f7a2a', 'orchard', 'food'],
    ['vineyard', 'map.layer.vineyard', '✽', '#6a2c8f', 'vineyard', 'food'],
    ['berries', 'map.layer.berries', '✿', '#3949ab', 'berries,perennial_veg', 'food'],
    ['fish', 'map.layer.fish', 'Fi', '#007d9c', 'fish_stocking', 'resource'],
    ['boat', 'map.layer.boat', 'B', '#176d43', 'boat_launch', 'resource'],
    ['mineral', 'map.layer.mineral', 'M', '#8b5429', 'mineral', 'resource'],
    ['well', 'map.layer.well', 'O', '#633ca6', 'oil_gas_well', 'resource'],
  ].map(([id, key, sym, col, cls, group]) => ({ id, key, sym, col, cls: cls.split(','), group }));
  const GROUPS = [['help', 'map.group.help'], ['water', 'map.group.water'], ['supply', 'map.group.supply'], ['food', 'map.group.food'], ['resource', 'map.group.resource']];
  const BY_CLS = {}; for (const l of LAYERS) for (const c of l.cls) BY_CLS[c] = l;
  const DEFAULT_LAYERS = ['hospital', 'clinic', 'pharmacy', 'fire', 'police', 'shelter', 'water', 'reservoir', 'fish', 'boat'];
  // Quick choices in the Layers panel: id, name, layers
  const PRESETS = [['emergency', 'map.preset.emergency', ['hospital', 'clinic', 'dentist', 'pharmacy', 'fire', 'police', 'shelter', 'community']], ['water', 'map.preset.water', ['water', 'spring', 'reservoir', 'pond', 'dam', 'fish', 'boat']],
    ['supplies', 'map.preset.supplies', ['food', 'drugstore', 'hardware', 'fuel', 'camp']], ['food', 'map.preset.food', ['orchard', 'vineyard', 'berries']], ['resources', 'map.preset.resources', ['fish', 'boat', 'mineral', 'well']], ['all', 'map.preset.all', LAYERS.map(l => l.id)]];
  const LEGACY = { all_resources: ['fish', 'boat', 'mineral', 'well'], mineral: ['mineral'], oil_gas_well: ['well'], fish_stocking: ['fish'], boat_launch: ['boat'] };
  // A record's fields, by the names its source gives them. (A field named year is the crop map's year: the dams' "Built" year was
  // listed first under the same name, and never shown.)
  const FIELD = { commod1: 'map.field.commodity', commod2: 'map.field.also', commod3: 'map.field.also', ore: 'map.field.ore', dev_stat: 'map.field.dev_stat', oper_type: 'map.field.operation', yr_lst_prd: 'map.field.last_production',
    county: 'map.field.district', state: 'map.field.region', WellTypeLabel: 'map.field.well_type', WellStatus: 'map.field.well_status', FieldName: 'map.field.oil_field', API: 'map.field.well_id', CountyName: 'map.field.district',
    FishType: 'map.field.fish', Counties: 'map.field.district', WeekOfPlantStart: 'map.field.stocked_week', Water_Body: 'map.field.water', Facility_Type: 'map.field.facility', County: 'map.field.district', Owner: 'map.field.owner',
    other_names: 'map.field.other_names', purpose: 'map.field.purpose', purposes: 'map.field.purposes', river: 'map.field.river', city: 'map.field.nearest_town', owner: 'map.field.owner',
    normal_storage_af: 'map.field.normal_storage', max_storage_af: 'map.field.max_storage', surface_acres: 'map.field.surface', height_ft: 'map.field.height',
    dam_type: 'map.field.dam_type', hazard: 'map.field.hazard', condition: 'map.field.condition', status: 'map.field.status', area_acres: 'map.field.area', water_type: 'map.field.water_present',
    reservoir_type: 'map.field.reservoir_type', crop: 'map.field.crop', crop_acres: 'map.field.crop_area', perennial_acres: 'map.field.perennial_area', year: 'map.field.crop_year',
    FUEL_SALES: 'map.field.fuel_sales', NO_OF_RAMP_LANE_USE_ONE_TIME: 'map.field.ramp_lanes', FISH_CLEANING: 'map.field.fish_cleaning', Comments: 'map.field.notes', Website: 'map.field.website' };
  // Fields a source records in US units, shown in metric: [factor, decimals]. Acre-feet to m³, acres to hectares, feet to metres.
  const METRIC = { normal_storage_af: [1233.48, 0], max_storage_af: [1233.48, 0], surface_acres: [0.404686, 1], area_acres: [0.404686, 1], crop_acres: [0.404686, 1], perennial_acres: [0.404686, 1], height_ft: [0.3048, 1] };
  const fieldValue = (k, v) => { const m = METRIC[k], n = Number(String(v).replace(/,/g, '')); return m && String(v).trim() !== '' && isFinite(n) ? I18N.num(n * m[0], { maximumFractionDigits: m[1] }) : v };
  const CAVEAT = { mineral: 'map.caveat.mineral', oil_gas_well: 'map.caveat.well', fish_stocking: 'map.caveat.fish', boat_launch: 'map.caveat.boat', dam: 'map.caveat.dam',
    reservoir: 'map.caveat.surface_water', lake: 'map.caveat.surface_water',
    orchard: 'map.caveat.crops', vineyard: 'map.caveat.crops', berries: 'map.caveat.crops', perennial_veg: 'map.caveat.crops',
    pond: 'map.caveat.pond', spring: 'map.caveat.spring', drinking_water: 'map.caveat.tap' };
  // A place with no name, said whole in each language ("Unnamed hospital"): by the server's class
  const UNNAMED = { hospital: 'map.unnamed.hospital', clinic: 'map.unnamed.clinic', doctors: 'map.unnamed.doctors', dentist: 'map.unnamed.dentist', pharmacy: 'map.unnamed.pharmacy', chemist: 'map.unnamed.chemist',
    fire_station: 'map.unnamed.fire_station', police: 'map.unnamed.police', shelter: 'map.unnamed.shelter', community_centre: 'map.unnamed.community_centre', town_hall: 'map.unnamed.town_hall', library: 'map.unnamed.library',
    drinking_water: 'map.unnamed.drinking_water', reservoir: 'map.unnamed.reservoir', lake: 'map.unnamed.lake', pond: 'map.unnamed.pond', dam: 'map.unnamed.dam', spring: 'map.unnamed.spring',
    supermarket: 'map.unnamed.supermarket', convenience: 'map.unnamed.convenience', hardware: 'map.unnamed.hardware', doityourself: 'map.unnamed.hardware', fuel: 'map.unnamed.fuel', camp_site: 'map.unnamed.camp_site',
    orchard: 'map.unnamed.orchard', vineyard: 'map.unnamed.vineyard', berries: 'map.unnamed.berries', perennial_veg: 'map.unnamed.perennial_veg',
    fish_stocking: 'map.unnamed.fish_stocking', boat_launch: 'map.unnamed.boat_launch', mineral: 'map.unnamed.mineral', oil_gas_well: 'map.unnamed.oil_gas_well' };
  // The server's direction from the map's centre ("north-east"), with the words each language puts before it
  const DIR = { north: 'map.dir.north', 'north-east': 'map.dir.north_east', east: 'map.dir.east', 'south-east': 'map.dir.south_east', south: 'map.dir.south', 'south-west': 'map.dir.south_west', west: 'map.dir.west', 'north-west': 'map.dir.north_west' };
  // "What this map covers": each row's code (crates/engine/src/map/info.rs CODES) → its label, and → its status. A row whose code
  // this page does not know shows the engine's English label and status.
  const INFO_LABEL = { base: 'map.info.base', streets_detail: 'map.info.streets_detail', streets_whole: 'map.info.streets_whole', no_detail: 'map.info.no_detail',
    places: 'map.info.places', addresses: 'map.info.addresses', elevation_contours: 'map.info.elevation_contours', elevation_broad: 'map.info.elevation_broad',
    buildings_detail: 'map.info.buildings_detail', buildings_whole: 'map.info.buildings_whole', crops_detail: 'map.info.crops_detail', crops_whole: 'map.info.crops_whole' };
  const INFO_STATUS = { base: 'map.info.base_status', streets_detail: 'map.info.streets_detail_status', streets_whole: 'map.info.streets_whole_status', no_detail: 'map.info.no_detail_status',
    places: 'map.info.places_status', addresses: 'map.info.addresses_status', elevation_contours: 'map.info.elevation_contours_status', elevation_broad: 'map.info.elevation_broad_status',
    buildings_detail: 'map.info.buildings_detail_status', buildings_whole: 'map.info.buildings_whole_status', crops_detail: 'map.info.crops_detail_status', crops_whole: 'map.info.crops_whole_status' };
  // A place's kind: by the address search's code (crates/engine/src/map/address KIND_CODES, kind_code), else by the kind the place
  // files give, lowercased (places.sqlite: city, town, village…; the water file: Lake, Reservoir, Dam, Pond, Water). Another kind shows as it comes.
  const KIND = { street_address: 'map.kind.street_address', street: 'map.kind.street', postcode: 'map.kind.postcode', town: 'map.kind.town', city: 'map.kind.city',
    village: 'map.kind.village', hamlet: 'map.kind.hamlet', suburb: 'map.kind.suburb', island: 'map.kind.island', county: 'map.kind.county', region: 'map.kind.region',
    lake: 'map.kind.lake', reservoir: 'map.kind.reservoir', dam: 'map.kind.dam', pond: 'map.kind.pond', water: 'map.kind.water',
    // the rest of what the place files hold (crates/tools/src/map/places.rs: Geofabrik's place classes, peaks, and detail.rs POI_KEEP)
    locality: 'map.kind.locality', isolated_dwelling: 'map.kind.isolated_dwelling', farm: 'map.kind.farm', municipality: 'map.kind.municipality', national_capital: 'map.kind.national_capital', peak: 'map.kind.peak',
    spring: 'map.kind.spring', beach: 'map.kind.beach', park: 'map.kind.park', hospital: 'map.kind.hospital', clinic: 'map.kind.clinic', doctors: 'map.kind.doctors',
    dentist: 'map.kind.dentist', pharmacy: 'map.kind.pharmacy', chemist: 'map.kind.chemist', veterinary: 'map.kind.veterinary', nursing_home: 'map.kind.nursing_home', fire_station: 'map.kind.fire_station',
    police: 'map.kind.police', courthouse: 'map.kind.courthouse', embassy: 'map.kind.embassy', town_hall: 'map.kind.town_hall', community_centre: 'map.kind.community_centre', library: 'map.kind.library',
    post_office: 'map.kind.post_office', school: 'map.kind.school', kindergarten: 'map.kind.kindergarten', college: 'map.kind.college', university: 'map.kind.university', shelter: 'map.kind.shelter',
    toilet: 'map.kind.toilet', drinking_water: 'map.kind.drinking_water', fountain: 'map.kind.fountain', water_tower: 'map.kind.water_tower', water_works: 'map.kind.water_works', wastewater_plant: 'map.kind.wastewater_plant',
    comms_tower: 'map.kind.comms_tower', supermarket: 'map.kind.supermarket', convenience: 'map.kind.convenience', doityourself: 'map.kind.doityourself', hardware: 'map.kind.hardware', garden_centre: 'map.kind.garden_centre',
    bicycle_shop: 'map.kind.bicycle_shop', car_repair: 'map.kind.car_repair', fuel: 'map.kind.fuel', camp_site: 'map.kind.camp_site' };
  // The line under a lake's or dam's name, after where it is: each part's code (crates/engine/src/map/places.rs PLACE_CODES) → its words
  const PLACE_PART = { also_called: 'map.place.also_called', km_away: 'map.place.km_away', nearby: 'map.place.nearby' };
  // A resource's line (crates/engine/src/map/pins.rs SUMMARY_CODES): its area, NOT drinking water, a crop's area in its block, what it
  // stores, a historical stocking record. A part with no code is the record's own word, shown as it comes.
  const PIN_PART = { area_ha: 'map.pin.area_ha', not_drinking_water: 'map.pin.not_drinking_water', crop_block_ha: 'map.pin.crop_block_ha',
    stored_m3: 'map.pin.stored_m3', stocking_record: 'map.pin.stocking_record' };
  // The credits under the map (crates/engine/src/map/info.rs CREDIT_CODES): OpenStreetMap in the interface's language, any other source
  // by the name its file gives. The outline of the world this page draws itself (world-context.json, geographic-context.json) is
  // Natural Earth's, so that credit is always there.
  const CREDIT = { osm: 'map.credit.osm' };
  const OUTLINE_CREDIT = 'Natural Earth';
  let st = null, world = null, geography = null;

  const load = (k, d) => { try { return JSON.parse(localStorage.getItem('sg.' + k)) ?? d } catch { return d } };
  const save = (k, v) => { try { localStorage.setItem('sg.' + k, JSON.stringify(v)) } catch { } };
  const lon2x = (lon, z) => (lon + 180) / 360 * 2 ** z;
  const lat2y = (lat, z) => { const s = Math.sin(Math.max(-85, Math.min(85, lat)) * Math.PI / 180); return (0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI)) * 2 ** z };
  const x2lon = (x, z) => x / 2 ** z * 360 - 180;
  const y2lat = (y, z) => { const n = Math.PI - 2 * Math.PI * y / 2 ** z; return 180 / Math.PI * Math.atan(Math.sinh(n)) };
  const escH = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const $ = id => document.getElementById(id);
  const dist = km => km < 1 ? I18N.unit(Math.round(km * 1000), 'meter') : I18N.unit(km, 'kilometer', km < 10 ? 1 : 0);
  const kmBetween = (a, b, c, d) => { const r = Math.PI / 180, x = Math.sin((c - a) * r / 2) ** 2 + Math.cos(a * r) * Math.cos(c * r) * Math.sin((d - b) * r / 2) ** 2; return 12742 * Math.asin(Math.sqrt(x)) };
  const sym = (l, size = 22) => `<i class="msym" style="background:${l.col};width:${size}px;height:${size}px">${escH(l.sym)}</i>`;
  const layerOf = cls => BY_CLS[cls] || { sym: '•', col: '#555', name: cls };
  const lname = l => l.key ? t(l.key) : l.name || '';   // a layer's name in the interface's language (a class the layers do not know keeps its own)
  const unnamed = p => p.name || p.detail?.reservoir_type || t(UNNAMED[p.cls] || 'map.unnamed.place');
  // One row of "What this map covers" in the interface's language: the source a map file names after the label; the regions of the
  // address search as the setup page names them, a country by Intl, a part of one by its own name (Bayern, Lombardia).
  function covers(d) {
    const p = d.params && typeof d.params === 'object' ? d.params : {}, names = Array.isArray(p.region_names) ? p.region_names : [];
    const msg = { code: d.code, params: { ...p, regions: Array.isArray(p.regions) ? I18N.list(p.regions.map((r, i) => /^[A-Z]{2}$/.test(r) ? I18N.country(r) : names[i] || r)) : '' } };
    const known = typeof d.code === 'string' && Object.hasOwn(INFO_LABEL, d.code), label = I18N.said(INFO_LABEL, msg, d.label);
    return { label: known && p.source ? t('map.info.with_source', { label, source: String(p.source) }) : label, status: I18N.said(INFO_STATUS, msg, d.status) };
  }
  const kindKey = k => typeof k === 'string' && Object.hasOwn(KIND, k) ? KIND[k] : null;
  const kindName = p => { const k = kindKey(p.kind_code) || kindKey(String(p.kind || '').trim().toLowerCase().replace(/[\s-]+/g, '_')); return k ? t(k) : p.kind || '' };
  // A line the engine says in parts (detail_parts, summary_parts): each part in the interface's language when this page knows its code,
  // else as the engine's English text gives it; a number in a part (km, ha, m3) written the way the language writes numbers
  const saidParts = (table, parts) => parts.map(d => I18N.said(table, d, d && d.text)).filter(Boolean).join(' · ');
  // The line about a resource (a lake's area, a dam's storage), from its parts; an engine that sends none gives its English line
  const summaryOf = (parts, english) => Array.isArray(parts) && parts.length ? saidParts(PIN_PART, parts) : english || '';
  // The line under a place's name: where it is (an address's town and postcode, a lake's municipality: locality) with its country in the
  // interface's language, then what more the engine says of it (detail_parts: its other names, how far it is). A place the engine sends
  // no locality for (the place files' own detail) keeps the engine's detail.
  const placeDetail = p => {
    if (typeof p.locality !== 'string') return p.detail || '';
    const where = [p.locality, /^[A-Z]{2}$/.test(p.country || '') ? I18N.country(p.country) : ''].filter(Boolean).join(', ');
    return [where, Array.isArray(p.detail_parts) ? saidParts(PLACE_PART, p.detail_parts) : ''].filter(Boolean).join(' · ');
  };
  // The note under the heading: the day the installed map's data was taken (its meta.data_as_of, "2026-11-16T21:22:36Z"), the way the
  // interface's language writes a day; a map that names no day gets the note that names none.
  const asOf = meta => {
    const m = /^(\d{4})-(\d\d)-(\d\d)/.exec(String(meta && meta.data_as_of || ''));
    return m ? th('map.offline_note', { date: I18N.date(new Date(+m[1], +m[2] - 1, +m[3]), { year: 'numeric', month: 'long', day: 'numeric' }) }) : th('map.offline_note_undated');
  };
  // The credits under the map, as HTML: each source the map's files name, with its licence (when its file names one) shown on hover
  function credits(list) {
    const shown = (Array.isArray(list) ? list : []).filter(c => c && typeof c === 'object')
      .map(c => ({ text: I18N.said(CREDIT, c, c.text), licence: typeof c.licence === 'string' ? c.licence : '' })).filter(c => c.text);
    if (!shown.some(c => c.text === OUTLINE_CREDIT)) shown.push({ text: OUTLINE_CREDIT, licence: '' });
    return shown.map(c => `<span${c.licence ? ` title="${escH(c.licence)}"` : ''}>${escH(c.text)}</span>`).join(' · ');
  }
  // Is there an address index for the map's own region? The panel's addresses row names the regions whose indexes are installed (a whole
  // country's index counts for its regions); a map that names no region of its own counts any index. Until the map has said, none is known.
  function addressesHere() {
    const have = st && st.addressRegions, region = st && st.region;
    if (!Array.isArray(have) || !have.length) return false;
    return !/^[A-Z]{2}(-[A-Z0-9]{1,3})?$/.test(region || '') || have.includes(region) || have.includes(region.split('-')[0]);
  }

  let askedHome = false;
  async function localHome() {
    if (!askedHome && !IN_WINDOW) { askedHome = true; try { const r = await fetch('/ui/home-area.json'); if (r.ok) HOME = await r.json() } catch { } }
    return HOME;
  }
  const wholeMap = () => st.bounds || EUROPE;
  async function page(main, q) {
    await localHome();
    const home = load('mapHome', HOME);
    const lat = parseFloat(q.get('lat')), lon = parseFloat(q.get('lon')), z = parseFloat(q.get('z'));
    let on = (q.get('layers') || '').split(',').filter(id => LAYERS.some(l => l.id === id));
    if (!on.length && LEGACY[q.get('kind')]) on = LEGACY[q.get('kind')];
    if (!on.length) on = (load('mapLayers', DEFAULT_LAYERS) || []).filter(id => LAYERS.some(l => l.id === id));  // drop retired layers (e.g. toilets)
    if (!on.length) on = DEFAULT_LAYERS;
    main.classList.add('map-page');
    document.title = t('map.page_title') + ' · Lastbook';
    main.innerHTML = `<div class="maphead"><h1>${th('map.heading')}</h1><p class="small muted" id="mnote">${asOf(null)}</p>${IN_WINDOW ? `<a class="btn plain" href="/ui/setup.html#manage">${th('map.downloads')}</a>` : ''}</div>
    <div class="mapgrid"><div class="mapwrap" id="mapwrap">
      <canvas id="mapc" tabindex="0" role="application" aria-label="${th('map.canvas_label')}"></canvas>
      <div class="map-top">
        <form class="map-search" id="pf" role="search"><label class="sr" for="pq">${th('map.find_label')}</label><input id="pq" type="search" placeholder="${th('map.find_placeholder')}" autocomplete="off"><button aria-label="${th('map.find_button')}"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg></button></form>
        <button class="map-layers-btn" id="layers-btn" aria-expanded="false" aria-controls="layers">${th('map.layers')} <b id="layer-count"></b></button>
      </div>
      <div class="map-pop" id="pres" hidden></div>
      <div class="map-pop layers" id="layers" hidden></div>
      <div class="mapctl"><span class="zgrp" role="group" aria-label="${th('map.zoom')}"><button id="zin" aria-label="${th('map.zoom_in')}" title="${th('map.zoom_in')}">+</button><button id="zout" aria-label="${th('map.zoom_out')}" title="${th('map.zoom_out')}">−</button></span>
        <button id="zhome" aria-label="${th('map.home_label')}" title="${th('map.home_title')}"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 11 12 4l9 7v9h-6v-6H9v6H3z"/></svg></button><button id="map-us"${t('map.whole_button').length > 5 ? ' class="long"' : ''} aria-label="${th('map.whole_map')}" title="${th('map.whole_map')}">${th('map.whole_button')}</button>
        <button id="map-full" aria-label="${th('map.full_screen')}" title="${th('map.full_screen')}">${FULL[0]}</button><button id="map-3d" aria-label="${th('map.view_3d_label')}" title="${th('map.view_3d_title')}">3D</button><button id="map-photo" aria-label="${th('map.aerial')}" aria-pressed="false" title="${th('map.aerial')}">🛰</button></div>
      <div class="map-card" id="card" hidden></div>
      <div class="map-tip" id="tip" hidden></div>
      <div class="map-status" id="mstatus" aria-live="polite"></div>
      <div class="mapelev" id="melev" hidden></div><div class="mapscale" id="mscale"></div><div class="mapattr" id="mattr">${credits([])}</div>
    </div>
    <div class="maptools">
      <section class="panel"><h2>${th('map.nearest_title')}</h2><p class="small muted" style="margin:0 0 4px">${th('map.nearest_note')}</p><div class="nearres" id="near" aria-live="polite"></div></section>
      <section class="panel"><h2>${th('map.area_title')}</h2><p class="small muted" style="margin:0 0 8px">${th('map.area_note')}</p><button class="btn plain" id="sethome">${th('map.set_home')}</button>${navigator.geolocation && isSecureContext ? ` <button class="btn plain" id="gps">${th('map.use_location')}</button>` : ''}</section>
      <section class="panel"><h2>${th('map.covers_title')}</h2><ul class="list small" id="mdata"><li class="muted">${th('map.loading')}</li></ul></section>
    </div></div>`;
    // The map fills the window. With the tools beside it, the page ends where the window does, at the page's own bottom margin (so the
    // page itself never scrolls); with the tools below, the map leaves room for the tab bar and a margin.
    const tools = main.querySelector('.maptools');
    const fitMap = () => {
      const w = $('mapwrap'); if (!w) return;
      const top = w.getBoundingClientRect().top + scrollY, tb = document.querySelector('.tabbar'), beside = tools.getBoundingClientRect().top < w.getBoundingClientRect().bottom;
      main.style.setProperty('--map-top', Math.round(top) + 'px');
      main.style.setProperty('--map-bottom', (beside ? parseFloat(getComputedStyle(main).paddingBottom) + parseFloat(getComputedStyle(document.body).paddingBottom) : (tb && tb.offsetParent ? tb.offsetHeight : 0) + 16) + 'px');
    };
    fitMap(); window.onresize = fitMap;   // the router clears window.onresize on the next page
    // The tools column scrolls on its own when it is taller than the map, and fades at an edge with more past it, as the chat list does
    const toolsEdge = () => { const more = tools.scrollHeight - tools.clientHeight; tools.classList.toggle('more-b', more > 1 && tools.scrollTop < more - 1); tools.classList.toggle('more-t', more > 1 && tools.scrollTop > 1) };
    tools.addEventListener('scroll', toolsEdge, { passive: true });
    const toolsSeen = new ResizeObserver(toolsEdge); toolsSeen.observe(tools); [...tools.children].forEach(c => toolsSeen.observe(c));
    const place = isFinite(lat) && isFinite(lon) ? null : home;
    const openWhole = !place && !(isFinite(lat) && isFinite(lon));   // nothing says where to open: the whole of this map, once its bounds have come
    st = { lat: isFinite(lat) ? lat : place ? place.lat : 50, lon: isFinite(lon) ? lon : place ? place.lon : 10, z: isFinite(z) ? Math.max(MINZ, Math.min(MAXZ, z)) : place ? place.z : 4,
      on: new Set(on), tiles: new Map(), pending: new Set(), gen: 0, mark: null, sel: null, raf: 0, alive: true, pins: [], pinBox: null, hits: [], serial: { pins: 0, near: 0, members: 0, place: 0 }, hover: null,
      dem: new Map(), demPending: new Set(), demNone: new Map(), style: load('mapStyle', { relief: true, contours: true }) };
    if (q.get('pin') && isFinite(lat) && isFinite(lon)) st.mark = { lat, lon, name: q.get('pin').slice(0, 80), kind: t('map.linked_place') };
    const cv = $('mapc');
    st.cv = cv; st.ctx = cv.getContext('2d');
    fetch('/ui/geographic-context.json').then(r => r.json()).then(d => { geography = d; draw() }).catch(() => { });
    fetch('/ui/world-context.json').then(r => r.json()).then(d => { world = d; draw() }).catch(() => { });
    const ro = new ResizeObserver(() => { draw(); if (st) { clearTimeout(st.resizeTimer); st.resizeTimer = setTimeout(() => loadPins(), 250) } }); ro.observe(cv); st.ro = ro;
    bind(cv);
    $('zin').onclick = () => zoomAt(1, cv.clientWidth / 2, cv.clientHeight / 2);
    $('zout').onclick = () => zoomAt(-1, cv.clientWidth / 2, cv.clientHeight / 2);
    $('zhome').onclick = () => { const h = load('mapHome', HOME); if (h) go(h.lat, h.lon, h.z); else { closeCard(); fit(wholeMap(), 0.92, true) } };
    $('map-us').onclick = () => { closeCard(); fit(wholeMap(), 0.92, true) };
    $('map-full').onclick = toggleFull;
    $('map-photo').onclick = () => setPhoto(!st.style.aerial);
    $('map-photo').classList.toggle('on', !!st.style.aerial); $('map-photo').setAttribute('aria-pressed', !!st.style.aerial);
    $('map-3d').onclick = () => {
      if (typeof Terrain3D === 'undefined') return toastMap(t('map.view_3d_unavailable'));
      closeCard();
      const w = cv.clientWidth, h = cv.clientHeight;
      Terrain3D.open($('mapwrap'), { lat: st.lat, lon: st.lon, z: st.z, w, h, texture: cv, elev: elevation, demTile: (z, x, y) => demTile(z, x, y), getDem: (z, x, y) => st && st.dem.get(tileKey(z, x, y)),
        geo: { lon2x, lat2y, x2lon, y2lat }, ver: st.ver, maxTz: tzMax(st.lat, st.lon) });
    };
    document.addEventListener('fullscreenchange', st.onFull = () => { const f = document.fullscreenElement === $('mapwrap'); if (!f) $('mapwrap')?.classList.remove('max'); syncFull() });
    $('sethome').onclick = () => { save('mapHome', { lat: st.lat, lon: st.lon, z: Math.round(st.z) }); toastMap(t('map.home_saved')); draw() };
    const gps = $('gps');
    if (gps) gps.onclick = () => navigator.geolocation.getCurrentPosition(p => { st.mark = { lat: p.coords.latitude, lon: p.coords.longitude, name: t('map.you_are_here') }; go(p.coords.latitude, p.coords.longitude, 15) }, e => toastMap(t('map.location_unavailable', { why: e.message })), { timeout: 15000 });
    $('pf').onsubmit = e => { e.preventDefault(); findPlace($('pq').value.trim()) };
    $('pq').addEventListener('input', () => { if (!$('pq').value) $('pres').hidden = true });
    $('layers-btn').onclick = () => { const p = $('layers'), open = p.hidden; p.hidden = !open; $('layers-btn').setAttribute('aria-expanded', open); $('pres').hidden = true; if (open) { $('card').hidden = true; renderLayers() } };
    st.onKey = e => { if (e.key === 'Escape' && st) { if (!$('layers').hidden) { $('layers').hidden = true; $('layers-btn').setAttribute('aria-expanded', 'false') } else if (!$('pres').hidden) $('pres').hidden = true; else if (!$('card').hidden) closeCard(); else if ($('mapwrap').classList.contains('max')) toggleFull() } };
    document.addEventListener('keydown', st.onKey);
    layerCount();
    // What the map's own files say of themselves: the day its data was taken (the note under the heading), who made it (the credits),
    // and whose addresses it can find (the place finder offers address search only for a region that has none)
    const own = info => {
      const rows = Array.isArray(info.datasets) ? info.datasets : [], addr = rows.find(d => d && d.code === 'addresses');
      st.addressRegions = addr && addr.params && Array.isArray(addr.params.regions) ? addr.params.regions : [];
      st.region = info.meta && typeof info.meta.region === 'string' ? info.meta.region : '';
      const note = $('mnote'), at = $('mattr');
      if (note) note.innerHTML = asOf(info.meta);
      if (at) at.innerHTML = credits(info.credits);
      fitMap();
    };
    fetch('/api/map/info').then(r => r.json()).then(info => {
      if (!st) return;
      st.national = info.national; st.ver = info.version; st.aerialDetail = info.aerial_detail; st.aerialRev = info.aerial_rev; st.terrainCircle = info.terrain_circle;
      own(info); describe(info); if (openWhole) fit(wholeMap(), 0.92, true); coverage(); draw();
      $('mdata').innerHTML = info.datasets.map(d => { const c = covers(d); return `<li class="row" style="display:block"><b>${escH(c.label)}</b><span class="muted">${escH(c.status)}</span>${d.bytes ? ` · ${I18N.unit(d.bytes / 1073741824, 'gigabyte', 1)}` : ''}</li>` }).join('') +
        (st.have && !st.have.size ? '' : `<li class="row small muted" style="display:block">${st.have ? '' : th('map.not_drawn') + ' '}${th('map.records_note')}</li>`);
    }).catch(() => { $('mdata').innerHTML = `<li class="muted">${th('map.details_unavailable')}</li>`; if (openWhole && st) fit(EUROPE, 0.92, true) });
    // When map data changes (national build, water import), quietly refresh visible tiles; old tiles stay up until new ones arrive.
    st.nationalTimer = setInterval(async () => {
      if (!st) return;
      try { const info = await fetch('/api/map/info').then(r => r.json()); if (!st) return; st.national = info.national; coverage();
        if (info.aerial_rev !== st.aerialRev) { st.aerialRev = info.aerial_rev; st.aerialDetail = info.aerial_detail; for (const v of st.aerial?.values() || []) v.close?.(); st.aerial = new Map(); draw() }  // new sharp photos: new URLs
        if (info.version !== st.ver) { st.ver = info.version; st.gen++; st.pinBox = null; own(info); draw(); loadPins() } } catch { }
    }, 60000);
    if (!openWhole) { draw(); settle() }
  }
  // What this map really holds, when the server says (an older one sends none of this, and everything stays offered).
  function describe(info) {
    if (info.aerial === false) { st.noAerial = true; st.style.aerial = false; $('map-photo')?.remove() }
    st.noContours = info.contours === false;
    if (!info.layers) return;
    st.have = new Set(info.layers); st.bounds = info.bounds;
    st.on = new Set([...st.on].filter(id => offered(LAYERS.find(l => l.id === id))));
    layerCount();
    if (!st.have.size) $('near').closest('section').hidden = true;
  }
  const offered = l => !st.have || (l && l.cls.some(c => st.have.has(c)));
  function toastMap(m) { if (typeof toast === 'function') toast(m) }
  function stop() {
    document.getElementById('main')?.classList.remove('map-page');
    if (typeof Terrain3D !== 'undefined') Terrain3D.close();
    if (st) { clearInterval(st.nationalTimer); for (const t of ['nt', 'wt', 'kt', 'resizeTimer']) clearTimeout(st[t]); cancelAnimationFrame(st.raf); st.alive = false; st.ro && st.ro.disconnect(); document.removeEventListener('keydown', st.onKey); document.removeEventListener('fullscreenchange', st.onFull); if (document.fullscreenElement) document.exitFullscreen().catch(() => { }); st = null }
  }
  function toggleFull() {
    const w = $('mapwrap'); if (!w) return;
    if (document.fullscreenElement) document.exitFullscreen().catch(() => { });
    else if (w.classList.contains('max')) w.classList.remove('max');
    else { w.classList.add('max'); w.requestFullscreen?.().catch(() => { }) }  // CSS fallback keeps working where the Fullscreen API is missing (iPhone)
    syncFull();
  }
  function syncFull() { const w = $('mapwrap'), b = $('map-full'); if (!w || !b) return; const on = w.classList.contains('max'); b.innerHTML = FULL[on ? 1 : 0]; b.setAttribute('aria-label', on ? t('map.exit_full_screen') : t('map.full_screen')); b.title = on ? t('map.exit_full_screen_esc') : t('map.full_screen'); document.body.classList.toggle('map-maxed', on); draw() }

  function coverage() {
    const n = st?.national, el = $('mstatus'); if (!el) return;
    const b = st.bounds, outside = b && (st.lon < b[0] || st.lon > b[2] || st.lat < b[1] || st.lat > b[3]);
    st.coverNote = n && n.state === 'building'
      ? t('map.cover.building', { n: I18N.num((n.features || 0) / 1e6, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })
      : outside ? t('map.cover.outside')
      : st.have && !st.have.size && st.z >= 9 ? t('map.cover.base_only') : '';
    statusLine();
  }
  function statusLine(msg) {
    const el = $('mstatus'); if (!el || !st) return;
    if (msg != null) st.pinMsg = msg;
    const parts = [st.pinMsg, st.coverNote].filter(Boolean);
    el.innerHTML = parts.map(escH).join('<br>'); el.hidden = !parts.length;
  }

  // ---------------------------------------------------------------- layers ----
  function kinds() { return LAYERS.filter(l => st.on.has(l.id)).flatMap(l => l.cls) }
  function layerCount() { const n = st.on.size; $('layer-count').textContent = st.have && !st.have.size ? '' : '· ' + (n ? I18N.num(n) : t('map.layers_none')) }
  function renderLayers() {
    const box = $('layers'); if (!box || box.hidden) return;
    const counts = {}; for (const p of st.pins) if (inView(p)) for (const [k, n] of Object.entries(p.kinds)) { const l = layerOf(k); counts[l.id] = (counts[l.id] || 0) + n }
    if (box.querySelector('fieldset')) {  // already built: refresh ticks and counts in place so focus isn't lost
      box.querySelectorAll('label.ly').forEach(lb => { const i = lb.querySelector('input'); i.checked = st.on.has(i.value); lb.querySelector('small').textContent = i.checked ? I18N.num(counts[i.value] || 0) : '' });
      return;
    }
    box.innerHTML = `<div class="layers-head"><b>${th('map.layers_title')}</b><button class="x" aria-label="${th('map.layers_close')}">×</button></div>
      ${st.have && !st.have.size ? '' : `<div class="presets">${PRESETS.map(([id, key]) => `<button data-p="${id}">${th(key)}</button>`).join('')}<button data-p="">${th('map.preset.none')}</button></div>`}
      <fieldset class="style"><legend>${th('map.style.legend')}</legend>
        ${st.noAerial ? '' : `<label><input type="checkbox" data-style="aerial"${st.style.aerial ? ' checked' : ''}><i class="msym style-ico">🛰</i><span>${th('map.aerial')}</span></label>`}
        <label><input type="checkbox" data-style="relief"${st.style.relief ? ' checked' : ''}><i class="msym style-ico">⛰</i><span>${th('map.style.relief')}</span></label>
        ${st.noContours ? '' : `<label><input type="checkbox" data-style="contours"${st.style.contours ? ' checked' : ''}><i class="msym style-ico">〰</i><span>${th('map.style.contours')}</span></label>`}
        ${st.have && !st.have.has('orchard') ? '' : `<label><input type="checkbox" data-style="crops"${st.style.crops !== false ? ' checked' : ''}><i class="msym style-ico" style="background:#3f7a2a">♣</i><span>${th('map.style.crops')}</span></label>`}</fieldset>
      ${GROUPS.filter(([g]) => LAYERS.some(l => l.group === g && offered(l))).map(([g, gk]) => `<fieldset><legend>${th(gk)}</legend>${LAYERS.filter(l => l.group === g && offered(l)).map(l => `<label class="ly"><input type="checkbox" value="${l.id}"${st.on.has(l.id) ? ' checked' : ''}>${sym(l, 20)}<span>${escH(lname(l))}</span><small>${st.on.has(l.id) ? I18N.num(counts[l.id] || 0) : ''}</small></label>`).join('')}</fieldset>`).join('')}
      ${st.have && !st.have.size ? '' : `<p class="small muted" style="margin:6px 2px 0">${th('map.layers_numbers')}</p>`}`;
    box.querySelector('.x').onclick = () => { box.hidden = true; $('layers-btn').setAttribute('aria-expanded', 'false') };
    box.querySelectorAll('label.ly input').forEach(i => i.onchange = () => { i.checked ? st.on.add(i.value) : st.on.delete(i.value); layersChanged() });
    box.querySelectorAll('[data-style]').forEach(i => i.onchange = () => { if (i.dataset.style === 'aerial') return setPhoto(i.checked); st.style[i.dataset.style] = i.checked; save('mapStyle', st.style); draw() });
    box.querySelectorAll('[data-p]').forEach(b => b.onclick = () => { st.on = new Set((PRESETS.find(p => p[0] === b.dataset.p)?.[2] || []).filter(id => offered(LAYERS.find(l => l.id === id)))); layersChanged(); renderLayers() });
    renderLayers();
  }
  function layersChanged() { save('mapLayers', [...st.on]); layerCount(); st.pinBox = null; if (st.sel && !st.on.has(layerOf(st.sel.cls).id)) closeCard(); settle() }

  // ----------------------------------------------------------------- input ----
  function bind(cv) {
    const pts = new Map(); let last = null, pinch = null, travel = 0;
    cv.addEventListener('pointerdown', e => { cv.setPointerCapture(e.pointerId); pts.set(e.pointerId, { x: e.offsetX, y: e.offsetY }); last = { x: e.offsetX, y: e.offsetY }; travel = 0; if (pts.size === 2) pinch = spread(); hideTip(); cv.classList.add('dragging') });
    cv.addEventListener('pointermove', e => {
      if (!pts.has(e.pointerId)) { hover(e.offsetX, e.offsetY); return }
      pts.set(e.pointerId, { x: e.offsetX, y: e.offsetY });
      if (pts.size === 2 && pinch) { const d = spread(), c = mid(); zoomAt(Math.log2(d / pinch), c.x, c.y, true); pinch = d; travel += 10; return }
      if (pts.size === 1 && last) { travel += Math.abs(e.offsetX - last.x) + Math.abs(e.offsetY - last.y); pan(e.offsetX - last.x, e.offsetY - last.y); last = { x: e.offsetX, y: e.offsetY } }
    });
    const up = e => { if (!pts.has(e.pointerId)) return; pts.delete(e.pointerId); if (pts.size < 2) pinch = null; last = pts.size === 1 ? [...pts.values()][0] : null; if (!pts.size) { cv.classList.remove('dragging'); if (travel > 4) settle() } };
    cv.addEventListener('pointerup', up); cv.addEventListener('pointercancel', up);
    cv.addEventListener('pointerleave', () => { if (!pts.size) { hideTip(); $('melev').hidden = true } });
    cv.addEventListener('wheel', e => { e.preventDefault(); hideTip(); zoomAt(-e.deltaY / (e.deltaMode ? 3 : (e.ctrlKey ? 100 : 300)), e.offsetX, e.offsetY, true); clearTimeout(st.wt); st.wt = setTimeout(settle, 250) }, { passive: false });
    // touch taps are a little forgiving: up to 10px of finger wobble still counts as a tap
    cv.addEventListener('click', e => { if (!st || travel > (e.pointerType === 'mouse' ? 4 : 10)) return; $('pres').hidden = true; if (!$('layers').hidden) { $('layers').hidden = true; $('layers-btn').setAttribute('aria-expanded', 'false') } const hit = hitAt(e.offsetX, e.offsetY, e.pointerType !== 'mouse'); if (hit) activate(hit); else if (!identifyCrop(e.offsetX, e.offsetY) && !$('card').hidden) closeCard() });
    cv.addEventListener('dblclick', e => { if (!hitAt(e.offsetX, e.offsetY)) zoomAt(1, e.offsetX, e.offsetY) });
    cv.addEventListener('keydown', e => {
      const k = e.key, s = 100;
      if (k === 'ArrowLeft') pan(s, 0); else if (k === 'ArrowRight') pan(-s, 0); else if (k === 'ArrowUp') pan(0, s); else if (k === 'ArrowDown') pan(0, -s);
      else if (k === '+' || k === '=') return zoomAt(1, cv.clientWidth / 2, cv.clientHeight / 2); else if (k === '-' || k === '_') return zoomAt(-1, cv.clientWidth / 2, cv.clientHeight / 2);
      else return; e.preventDefault(); clearTimeout(st.kt); st.kt = setTimeout(settle, 200);
    });
    function spread() { const [a, b] = [...pts.values()]; return Math.hypot(a.x - b.x, a.y - b.y) || 1 }
    function mid() { const [a, b] = [...pts.values()]; return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } }
  }
  const CROP_RGB = {"Almonds":[150,100,50],"Walnuts":[120,80,40],"Pecans":[135,90,45],"Pistachios":[140,160,60],"Cherries":[190,30,60],"Peaches":[240,140,90],"Apples":[200,40,40],"Pears":[170,190,60],"Prunes":[110,50,120],"Nectarines":[240,120,70],"Plums":[130,40,110],"Apricots":[245,160,50],"Pomegranates":[180,30,50],"Avocados":[70,120,40],"Olives":[110,120,50],"Other tree crops":[90,150,70],"Citrus":[250,160,20],"Oranges":[255,140,0],"Grapes":[120,50,150],"Blueberries":[60,70,180],"Cranberries":[170,20,50],"Caneberries":[150,30,90],"Asparagus":[90,160,90]};  // overlay colours, as the crop map is drawn
  // Clicking a coloured orchard / vineyard patch opens the crop block under it.
  function identifyCrop(x, y) {
    if (st.style.crops === false || st.z < 11) return false;
    const tz = Math.min(13, Math.floor(st.z + 0.01)), w = st.cv.clientWidth, h = st.cv.clientHeight;
    const fx = lon2x(st.lon, tz) + (x - w / 2) / 256 / 2 ** (st.z - tz), fy = lat2y(st.lat, tz) + (y - h / 2) / 256 / 2 ** (st.z - tz);
    const img = st.crops?.get(tileKey(tz, Math.floor(fx), Math.floor(fy))); if (!img) return false;
    const c = document.createElement('canvas'); c.width = c.height = 1; const g = c.getContext('2d');
    g.drawImage(img, Math.floor((fx % 1) * 256), Math.floor((fy % 1) * 256), 1, 1, 0, 0, 1, 1);
    const px = g.getImageData(0, 0, 1, 1).data; if (px[3] < 40) return false;
    const crop = Object.entries(CROP_RGB).sort((a, b) => Math.hypot(...a[1].map((v, i) => v - px[i])) - Math.hypot(...b[1].map((v, i) => v - px[i])))[0][0];
    const lon = x2lon(fx, tz), lat = y2lat(fy, tz), d = 0.006, state = st;
    fetch('/api/map/members?' + new URLSearchParams({ west: lon - d, south: lat - d, east: lon + d, north: lat + d, lat, lon, kinds: 'orchard,vineyard,berries,perennial_veg' }))
      .then(r => r.json()).then(r => { const m = r.items?.find(i => i.detail?.crop === crop) || r.items?.[0]; if (st === state && m) showDetail(m) }).catch(() => { });
    return true;
  }
  function cursorElev(x, y) {
    const el = $('melev'); if (!el) return;
    const w = st.cv.clientWidth, h = st.cv.clientHeight;
    const lon = x2lon(lon2x(st.lon, st.z) + (x - w / 2) / 256, st.z), lat = y2lat(lat2y(st.lat, st.z) + (y - h / 2) / 256, st.z);
    const m = elevAt(lat, lon);
    el.hidden = m == null; if (m != null) el.textContent = '⛰ ' + t('map.elev_here', { elev: fmtElev(m) });
  }
  function hover(x, y) {
    const hit = hitAt(x, y); st.cv.style.cursor = hit ? 'pointer' : '';
    if (!hit) cursorElev(x, y); else $('melev').hidden = true;
    if (hit === st.hover) { if (hit) placeTip(x, y); return }
    st.hover = hit; const tip = $('tip'); if (!tip) return;
    if (!hit) return hideTip();
    tip.innerHTML = hit.count > 1 && !hit.spot ? `<b>${th('map.n_places', { n: hit.count })}</b>${breakdown(hit)}<small>${st.z >= 16 ? th('map.tip.click_list') : th('map.tip.click_zoom')}</small>`
      : hit.count > 1 ? `<b>${th('map.n_records_spot', { n: hit.count })}</b><small>${th('map.tip.spot_list', { kind: lname(layerOf(hit.cls)) })}</small>`
      : `<b>${escH(hit.place ? hit.name : unnamed(hit))}</b><small>${escH(hit.place ? hit.kind || t('map.place') : lname(layerOf(hit.cls)))}${hit.records > 1 ? ' · ' + th('map.tip.records', { n: hit.records }) : ''}</small>`;
    tip.hidden = false; placeTip(x, y);
  }
  function breakdown(p) {
    const by = {}; for (const [k, n] of Object.entries(p.kinds || {})) { const l = layerOf(k); by[l.id || k] = by[l.id || k] || [l, 0]; by[l.id || k][1] += n }
    const e = Object.values(by).sort((a, b) => b[1] - a[1]);
    return e.length > 1 ? `<span class="mix">${e.slice(0, 6).map(([l, n]) => `${sym(l, 14)}${th('map.breakdown_row', { kind: lname(l).replace(/ \(.*\)/, ''), n })}`).join('<br>')}</span>` : '';
  }
  function placeTip(x, y) { const tip = $('tip'), w = st.cv.clientWidth; tip.style.left = Math.min(x + 14, w - tip.offsetWidth - 6) + 'px'; tip.style.top = Math.max(6, y - tip.offsetHeight - 10) + 'px' }
  function hideTip() { if (st) st.hover = null; const t = $('tip'); if (t) t.hidden = true }

  function pan(dx, dy) {
    const zf = st.z, wx = lon2x(st.lon, zf) - dx / 256, wy = lat2y(st.lat, zf) - dy / 256;
    st.lon = ((x2lon(wx, zf) + 540) % 360) - 180; st.lat = Math.max(-80, Math.min(80, y2lat(wy, zf))); draw();
  }
  function zoomAt(dz, px, py, smooth) {
    const cv = st.cv, w = cv.clientWidth, h = cv.clientHeight;
    const nz = Math.max(MINZ, Math.min(MAXZ, smooth ? st.z + dz : Math.round(st.z + dz)));
    const glon = x2lon(lon2x(st.lon, st.z) + (px - w / 2) / 256, st.z), glat = y2lat(lat2y(st.lat, st.z) + (py - h / 2) / 256, st.z);
    st.z = nz;
    st.lon = x2lon(lon2x(glon, nz) - (px - w / 2) / 256, nz); st.lat = y2lat(lat2y(glat, nz) - (py - h / 2) / 256, nz);
    if (!smooth) settle(); else draw();
  }
  function go(lat, lon, z) { Object.assign(st, { lat, lon, z: Math.max(MINZ, Math.min(MAXZ, z)) }); settle() }
  function fit(b, share = 0.7, free) {  // [west, south, east, north] into part of the view
    const w = st.cv.clientWidth * share, h = st.cv.clientHeight * share;
    const dx = Math.max(1e-9, lon2x(b[2], 0) - lon2x(b[0], 0)), dy = Math.max(1e-9, lat2y(b[1], 0) - lat2y(b[3], 0));
    const z = Math.min(Math.log2(w / 256 / dx), Math.log2(h / 256 / dy), 17);
    const cy = y2lat((lat2y(b[1], 0) + lat2y(b[3], 0)) / 2, 0);
    go(cy, (b[0] + b[2]) / 2, free ? Math.max(MINZ, z) : Math.min(17, Math.max(Math.floor(z * 2) / 2, st.z + 1.5)));
  }
  function settle(keepList) {
    if (!st) return;
    draw(); coverage();
    history.replaceState(null, '', `#/map?lat=${st.lat.toFixed(5)}&lon=${st.lon.toFixed(5)}&z=${st.z.toFixed(2)}&layers=${[...st.on].join(',')}`);  // a pin= marker belongs to the visit that linked it
    if (!keepList) st.listAt = null;
    clearTimeout(st.nt); st.nt = setTimeout(() => { if (!st) return; if (!st.listAt) nearby(); loadPins() }, 200);
  }

  // ----------------------------------------------------------------- tiles ----
  function tileKey(z, x, y) { return z + '/' + x + '/' + y }
  function getTile(z, x, y) {
    const k = tileKey(z, x, y), t = st.tiles.get(k);
    const stale = !t || (t.gen < st.gen) || (t.failed && Date.now() - t.at > 5000) || (t.canvas && t.photo !== !!st.style.aerial);
    if (stale && !st.pending.has(k) && st.pending.size < 8) {
      st.pending.add(k); const gen = st.gen, state = st;
      fetch('/api/map/tile/' + k + (st.ver ? '?v=' + st.ver : gen ? '?g=' + gen : '')).then(r => { if (!r.ok) throw Error(r.status); return r.json() })
        .then(d => { if (st !== state) return; st.pending.delete(k); st.tiles.delete(k); st.tiles.set(k, { ...render(d, z), gen, photo: !!st.style.aerial }); prune(); draw() })
        .catch(() => { if (st === state) { st.pending.delete(k); if (!t?.canvas) st.tiles.set(k, { failed: true, at: Date.now(), gen }); draw() } });
    }
    return t;
  }
  function prune() { if (st.tiles.size > 260) { const ks = [...st.tiles.keys()]; for (const k of ks.slice(0, ks.length - 200)) st.tiles.delete(k) } }

  // Each tile is drawn once into an offscreen canvas; labels are collected for screen-space placement.
  function render(d, z) {
    const R = Math.min(2, devicePixelRatio || 1), S = 256 * R, k = S / EXT;
    const c = document.createElement('canvas'); c.width = c.height = S;
    const g = c.getContext('2d'); const L = d.layers || {};
    const poly = (arr, fill, stroke) => { if (!arr) return; for (const [cls, , parts] of arr) { g.beginPath(); for (const p of parts) { g.moveTo(p[0] * k, p[1] * k); for (let i = 2; i < p.length; i += 2) g.lineTo(p[i] * k, p[i + 1] * k); g.closePath() } g.fillStyle = typeof fill === 'function' ? fill(cls) : fill; g.fill('evenodd'); if (stroke) { g.strokeStyle = stroke; g.lineWidth = 0.6 * R; g.stroke() } } };
    const line = (p) => { g.beginPath(); g.moveTo(p[0] * k, p[1] * k); for (let i = 2; i < p.length; i += 2) g.lineTo(p[i] * k, p[i + 1] * k) };
    const photo = st.style.aerial;  // over aerial photos only roads, rail and names are drawn, lighter
    if (!photo) {
      poly(L.ocean, C.ocean); poly(L.island, C.land);
      poly(L.land, cls => C[cls] || C.res); poly(L.beach, C.beach); poly(L.wet, C.wet); poly(L.water, C.water);
      if (L.waterway) for (const [cls, , parts] of L.waterway) { g.strokeStyle = C.waterway; g.lineWidth = (cls === 'river' ? (z < 9 ? 1.6 : 3) : z < 12 ? 0.7 : 1.2) * R; for (const p of parts) { line(p); g.stroke() } }
      if (z >= 15) poly(L.bldg, C.bldg, C.bldgLine);
    }
    g.globalAlpha = photo ? 0.55 : 1;
    const scale = Math.max(0.5, Math.min(2.4, 1 + (z - 14) * 0.35));
    const roads = (L.road || []).slice().sort((a, b) => roadStyle(a[0])[1] - roadStyle(b[0])[1]);
    g.lineCap = 'round'; g.lineJoin = 'round';
    for (const pass of [0, 1]) for (const [cls, , parts] of roads) {
      const [col, w] = roadStyle(cls); if ((w < 1.3 || z < 10) && pass === 0) continue;
      g.strokeStyle = pass ? col : '#cbbfae'; g.lineWidth = (pass ? w : w + 1.6) * scale * R * (z < 12 ? 0.6 : 1) * (photo ? 0.6 : 1);
      if (z < 10 && pass && col === '#ffffff') g.strokeStyle = '#d8d0c4';  // white minor roads vanish on beige at low zoom
      if (/footway|path|cycleway|steps|bridleway|track/.test(cls)) { if (!pass || photo) continue; g.setLineDash([3 * R, 2 * R]) } else g.setLineDash([]);
      if (photo && !pass) continue;
      for (const p of parts) { line(p); g.stroke() }
    }
    g.setLineDash([]);
    if (L.rail) { g.strokeStyle = C.rail; g.lineWidth = 1.4 * R; g.setLineDash([5 * R, 3 * R]); for (const [, , parts] of L.rail) for (const p of parts) { line(p); g.stroke() } g.setLineDash([]) }
    g.globalAlpha = 1;
    const labels = [];
    if (L.place) for (const [cls, name, parts] of L.place) labels.push({ x: parts[0][0] / EXT, y: parts[0][1] / EXT, name, cls, pri: { city: 1, town: 2, suburb: 3, village: 3, county: 0, region: 0 }[cls] ?? 4 });
    if (z >= 15 && L.road) for (const [cls, name, parts] of L.road) { if (!name || roadStyle(cls)[1] < 2) continue; const p = parts[0]; if (p.length < 4) continue; const i = (p.length / 4 | 0) * 2; labels.push({ x: p[i] / EXT, y: p[i + 1] / EXT, name, cls: 'road', pri: 6, ang: Math.atan2(p[i + 3] - p[i + 1], p[i + 2] - p[i]) }) }
    return { canvas: c, labels };
  }

  function draw() {
    if (!st || !st.alive) return;
    cancelAnimationFrame(st.raf);
    st.raf = requestAnimationFrame(paint);
  }
  const px = (lat, lon, V) => [V.w / 2 + (lon2x(lon, V.tz) - V.cx) * V.ts, V.h / 2 + (lat2y(lat, V.tz) - V.cy) * V.ts];
  function outline(g, fc, V) {
    for (const feature of fc.features) {
      const polys = feature.geometry.type === 'Polygon' ? [feature.geometry.coordinates] : feature.geometry.coordinates;
      g.beginPath();
      for (const poly of polys) for (const ring of poly) { ring.forEach(([lo, la], i) => { const [x, y] = px(la, lo, V); i ? g.lineTo(x, y) : g.moveTo(x, y) }); g.closePath() }
      feature.properties?.kind === 'lake' ? (g.fillStyle = C.water, g.fill('evenodd')) : fc === world ? (g.fill('evenodd'), g.stroke()) : g.stroke();
    }
  }
  function paint() {
    if (!st) return;
    const cv = st.cv, R = devicePixelRatio || 1, w = cv.clientWidth, h = cv.clientHeight;
    if (!w || !h) return;
    if (cv.width !== Math.round(w * R) || cv.height !== Math.round(h * R)) { cv.width = Math.round(w * R); cv.height = Math.round(h * R) }
    const g = st.ctx; g.setTransform(R, 0, 0, R, 0, 0);
    g.fillStyle = C.ocean; g.fillRect(0, 0, w, h);
    const tz = Math.max(MINZ, Math.min(DATAZ, Math.floor(st.z + 0.01)));
    const ts = 256 * 2 ** (st.z - tz), cx = lon2x(st.lon, tz), cy = lat2y(st.lat, tz);
    const V = { w, h, tz, ts, cx, cy };
    if (world) { g.fillStyle = C.land; g.strokeStyle = '#aab5a5'; g.lineWidth = .7; outline(g, world, V) }
    if (geography) { g.strokeStyle = '#a3aca1'; g.lineWidth = 1; outline(g, geography, V) }
    if (st.style.aerial) aerialLayer(g, V);
    const x0 = Math.floor(cx - w / 2 / ts), x1 = Math.floor(cx + w / 2 / ts), y0 = Math.max(0, Math.floor(cy - h / 2 / ts)), y1 = Math.min(2 ** tz - 1, Math.floor(cy + h / 2 / ts));
    const order = []; for (let ty = y0; ty <= y1; ty++) for (let tx = x0; tx <= x1; tx++) order.push([tx, ty]);
    order.sort((a, b) => Math.hypot(a[0] + .5 - cx, a[1] + .5 - cy) - Math.hypot(b[0] + .5 - cx, b[1] + .5 - cy));  // centre tiles load first
    const labels = [];
    let loading = 0;
    for (const [tx, ty] of order) {
      const n = 2 ** tz, wx = ((tx % n) + n) % n, sx = w / 2 + (tx - cx) * ts, sy = h / 2 + (ty - cy) * ts;
      const t = getTile(tz, wx, ty);
      if (t && t.canvas) {
        g.drawImage(t.canvas, sx, sy, ts + 0.5, ts + 0.5);
        for (const l of t.labels) labels.push({ ...l, sx: sx + l.x * ts, sy: sy + l.y * ts });
      } else {
        if (!t?.failed) loading++;
        for (let up = 1; up <= 5 && tz - up >= MINZ; up++) {  // a lower-zoom parent stands in while loading
          const pz = tz - up, f = 2 ** up, ppx = Math.floor(wx / f), ppy = Math.floor(ty / f), pt = st.tiles.get(tileKey(pz, ppx, ppy));
          if (pt && pt.canvas && pt.photo === !!st.style.aerial) { const S = pt.canvas.width / f; g.drawImage(pt.canvas, (wx - ppx * f) * S, (ty - ppy * f) * S, S, S, sx, sy, ts + 0.5, ts + 0.5); break }
        }
      }
    }
    st.loading = loading;
    if (st.style.crops !== false) cropLayer(g, V);
    if (!st.style.aerial) terrainLayer(g, V);
    else if (st.style.contours) terrainLayer(g, V, true);
    labels.sort((a, b) => a.pri - b.pri);
    const boxes = [];
    // pins first reserve their own space so labels don't sit on top of them
    for (const p of st.pins) { const [x, y] = px(p.lat, p.lon, V); p.sx = x; p.sy = y; p.r = p.count > 1 && !p.spot ? Math.min(21, 12 + Math.log10(p.count) * 3) : 11; boxes.push({ x: x - p.r, y: y - p.r, w: p.r * 2, h: p.r * 2 }) }
    g.textAlign = 'center'; g.textBaseline = 'middle';
    for (const l of labels) {
      if (!l.name || l.sx < -50 || l.sx > w + 50 || l.sy < -20 || l.sy > h + 20) continue;
      const size = l.cls === 'road' ? 11 : l.pri <= 1 ? 16 : l.pri <= 2 ? 14 : 13;
      g.font = `${l.cls === 'road' ? 500 : 700} ${size}px -apple-system,Segoe UI,Roboto,sans-serif`;
      const tw = g.measureText(l.name).width + 6, bx = { x: l.sx - tw / 2, y: l.sy - size / 2 - 2, w: tw, h: size + 4 };
      if (l.cls !== 'road' && boxes.some(b => bx.x < b.x + b.w && bx.x + bx.w > b.x && bx.y < b.y + b.h && bx.y + bx.h > b.y)) continue;
      if (l.cls === 'road' && boxes.slice(st.pins.length).some(b => bx.x < b.x + b.w && bx.x + bx.w > b.x && bx.y < b.y + b.h && bx.y + bx.h > b.y)) continue;
      boxes.push(bx);
      g.save(); g.translate(l.sx, l.sy);
      if (l.ang != null) { let a = l.ang; if (a > Math.PI / 2) a -= Math.PI; if (a < -Math.PI / 2) a += Math.PI; g.rotate(a) }
      g.lineWidth = 3.5; g.strokeStyle = C.halo; g.strokeText(l.name, 0, 0); g.fillStyle = l.cls === 'road' ? '#4a4a4a' : C.label; g.fillText(l.name, 0, 0); g.restore();
    }
    // pins: clusters underneath, single places on top; hit-testing walks the same list backwards
    st.hits = [];
    const sorted = st.pins.filter(p => p.sx > -40 && p.sx < w + 40 && p.sy > -40 && p.sy < h + 40).sort((a, b) => (b.count > 1 && !b.spot) - (a.count > 1 && !a.spot) || b.count - a.count);
    const nameBoxes = sorted.map(p => ({ x: p.sx - p.r, y: p.sy - p.r, w: p.r * 2, h: p.r * 2 }));  // names never cover another pin
    for (const p of sorted) {
      const l = layerOf(p.cls), selected = st.sel && Math.abs(st.sel.lat - p.lat) < 1e-7 && Math.abs(st.sel.lon - p.lon) < 1e-7;
      g.shadowColor = 'rgba(0,0,0,.28)'; g.shadowBlur = 3; g.shadowOffsetY = 1;
      if (p.count > 1 && !p.spot) {
        const kinds = Object.entries(p.kinds), tot = p.count;
        let a = -Math.PI / 2;
        if (kinds.length === 1) { g.beginPath(); g.arc(p.sx, p.sy, p.r, 0, 7); g.fillStyle = l.col; g.fill() }
        else for (const [k, n] of kinds.sort((x, y) => y[1] - x[1])) { const b = a + n / tot * Math.PI * 2; g.beginPath(); g.moveTo(p.sx, p.sy); g.arc(p.sx, p.sy, p.r, a, Math.max(b, a + 0.08)); g.closePath(); g.fillStyle = layerOf(k).col; g.fill(); a = b }
        g.shadowColor = 'transparent';
        g.beginPath(); g.arc(p.sx, p.sy, p.r - 5, 0, 7); g.fillStyle = kinds.length === 1 ? l.col : '#263238'; g.fill();
        g.lineWidth = 2; g.strokeStyle = '#fff'; g.beginPath(); g.arc(p.sx, p.sy, p.r, 0, 7); g.stroke();
        g.fillStyle = '#fff'; g.font = '700 12px -apple-system,Segoe UI,sans-serif';
        g.fillText(p.count >= 10000 ? Math.round(p.count / 1000) + 'k' : p.count >= 1000 ? (p.count / 1000).toFixed(1) + 'k' : p.count, p.sx, p.sy + 0.5);
      } else {
        if (selected) { g.beginPath(); g.arc(p.sx, p.sy, 18, 0, 7); g.fillStyle = 'rgba(255,193,7,.55)'; g.fill() }
        g.beginPath(); g.arc(p.sx, p.sy, p.r, 0, 7); g.fillStyle = l.col; g.fill(); g.shadowColor = 'transparent';
        g.lineWidth = 2; g.strokeStyle = '#fff'; g.stroke();
        g.fillStyle = '#fff'; g.font = `700 ${l.sym.length > 1 ? 9 : 11}px -apple-system,Segoe UI,sans-serif`; g.fillText(l.sym, p.sx, p.sy + 0.5);
        const badge = p.spot ? p.count : p.records > 1 ? p.records : 0;  // several records at one spot (e.g. repeated stocking)
        if (badge) { g.beginPath(); g.arc(p.sx + 9, p.sy - 9, 7.5, 0, 7); g.fillStyle = '#263238'; g.fill(); g.fillStyle = '#fff'; g.font = '700 9px -apple-system,sans-serif'; g.fillText(badge > 99 ? '99+' : badge, p.sx + 9, p.sy - 8.5) }
        p.labelWidth = 0;
        if (st.z >= 15 && p.name && p.count === 1) {
          g.font = '600 12px -apple-system,Segoe UI,sans-serif'; const tw = g.measureText(p.name).width, nb = { x: p.sx + 13, y: p.sy - 8, w: tw + 4, h: 16 };
          if (!nameBoxes.some(b => nb.x < b.x + b.w && nb.x + nb.w > b.x && nb.y < b.y + b.h && nb.y + nb.h > b.y)) {
            nameBoxes.push(nb); p.labelWidth = tw;
            g.textAlign = 'left'; g.lineWidth = 3; g.strokeStyle = C.halo; g.strokeText(p.name, p.sx + 14, p.sy); g.fillStyle = l.col; g.fillText(p.name, p.sx + 14, p.sy); g.textAlign = 'center';
          }
        }
      }
      g.shadowColor = 'transparent';
      st.hits.push(p);
    }
    if (st.mark) {
      const [mx, my] = px(st.mark.lat, st.mark.lon, V);
      g.beginPath(); g.moveTo(mx, my); g.bezierCurveTo(mx - 3, my - 9, mx - 10, my - 13, mx - 10, my - 21); g.arc(mx, my - 21, 10, Math.PI, 0); g.bezierCurveTo(mx + 10, my - 13, mx + 3, my - 9, mx, my);
      g.fillStyle = '#1565c0'; g.fill(); g.lineWidth = 2; g.strokeStyle = '#fff'; g.stroke(); g.beginPath(); g.arc(mx, my - 21, 4, 0, 7); g.fillStyle = '#fff'; g.fill();
      if (st.mark.name) { g.font = '700 13px -apple-system,Segoe UI,sans-serif'; g.textAlign = 'left'; g.lineWidth = 4; g.strokeStyle = C.halo; g.strokeText(st.mark.name, mx + 14, my - 20); g.fillStyle = '#0d3c78'; g.fillText(st.mark.name, mx + 14, my - 20); g.textAlign = 'center' }
      st.hits.push({ ...st.mark, place: true, count: 1, sx: mx, sy: my - 18, r: 12, labelWidth: st.mark.name ? st.mark.name.length * 7.5 : 0 });
    }
    // home marker
    const home = load('mapHome', HOME); if (home) { const [hx, hy] = px(home.lat, home.lon, V); if (hx > -10 && hx < w + 10 && hy > -10 && hy < h + 10) { g.beginPath(); g.arc(hx, hy, 6, 0, 7); g.fillStyle = '#fff'; g.fill(); g.lineWidth = 3; g.strokeStyle = '#1b5e20'; g.stroke() } }
    g.strokeStyle = 'rgba(0,0,0,.3)'; g.lineWidth = 1.5; g.beginPath(); g.moveTo(w / 2 - 7, h / 2); g.lineTo(w / 2 + 7, h / 2); g.moveTo(w / 2, h / 2 - 7); g.lineTo(w / 2, h / 2 + 7); g.stroke();
    const mpp = 156543.03 * Math.cos(st.lat * Math.PI / 180) / 2 ** st.z;
    const nice = [10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000, 100000, 200000, 500000, 1000000, 2000000].find(m => m / mpp > 60) || 100000;
    const el = $('mscale'); if (el) el.innerHTML = `<span style="display:inline-block;width:${Math.round(nice / mpp)}px;border:2px solid #333;border-top:0;height:6px;vertical-align:middle"></span> ${nice >= 1000 ? I18N.unit(nice / 1000, 'kilometer') : I18N.unit(nice, 'meter')}`;   // metric only
    const at = el && el.parentNode.querySelector('.mapattr'); if (at) at.style.maxWidth = Math.max(120, el.parentNode.clientWidth - el.offsetWidth - 34) + 'px';   // the credits wrap before they reach the scale bar, however long it is
    const ms = $('mstatus'); if (ms && at) ms.style.bottom = at.offsetHeight ? Math.max(34, at.offsetHeight + 14) + 'px' : '';   // the status line stays above the credits, however many lines they take
  }

  function hitAt(x, y, touch) {
    if (!st) return null; const slop = touch ? 10 : 4;
    for (let i = st.hits.length - 1; i >= 0; i--) { const p = st.hits[i]; if (Math.hypot(x - p.sx, y - p.sy) < p.r + slop) return p }
    for (let i = st.hits.length - 1; i >= 0; i--) { const p = st.hits[i]; if (p.labelWidth && x >= p.sx + 10 && x <= p.sx + 16 + p.labelWidth && Math.abs(y - p.sy) < 10) return p }
    return null;
  }
  function activate(p) {
    hideTip();
    if (p.place) return showPlace(p);
    if (p.count > 1 && !p.spot && st.z < 16) return fit(p.bbox);
    if (p.count > 1 || p.records > 1) return showMembers({ ...p, spot: p.spot || p.count === 1 });
    showDetail(p);
  }

  // --------------------------------------------------------------- terrain ----
  // Elevation tiles (/api/map/terrain/z/x/y.png, terrain-RGB) drive shaded relief, contours and spot heights:
  // zoom 4-12 everywhere (30 m), zoom 13-17 from 1 m lidar inside the detail area (/api/map/info terrain_detail = [w, s, e, n]).
  const TZ0 = 4, TZN = 12, TZD = 17;
  const tzMax = (lat, lon) => {  // lidar zooms only inside the built circle (/api/map/info terrain_circle = [lat, lon, km])
    const c = st.terrainCircle; if (!c) return TZN;
    const dy = (lat - c[0]) * 111.32, dx = (lon - c[1]) * 111.32 * Math.cos(c[0] * Math.PI / 180);
    return Math.hypot(dx, dy) <= c[2] ? TZD : TZN;
  };
  function demTile(z, x, y) {
    const k = tileKey(z, x, y); let t = st.dem.get(k);
    if (t === null && z > TZN && Date.now() - (st.demNone.get(k) || 0) > 60000) { st.dem.delete(k); t = undefined }  // lidar may still be building
    if (t !== undefined) { if (t) { st.dem.delete(k); st.dem.set(k, t) } return t }  // keep recently used tiles last (eviction is oldest-first)
    if (!st.demPending.has(k) && st.demPending.size < 6) {
      st.demPending.add(k); const state = st;
      fetch(`/api/map/terrain/${k}.png${st.ver ? '?v=' + st.ver : ''}`).then(r => r.status === 200 ? r.blob() : null)
        .then(b => b && createImageBitmap(b, { colorSpaceConversion: 'none', premultiplyAlpha: 'none' }))
        .then(img => {
          if (st !== state) return; st.demPending.delete(k);
          if (!img) { st.dem.set(k, null); st.demNone.set(k, Date.now()); return }
          const c = document.createElement('canvas'); c.width = c.height = 256; const cx = c.getContext('2d', { willReadFrequently: true });
          cx.drawImage(img, 0, 0); const d = cx.getImageData(0, 0, 256, 256).data, h = new Float32Array(65536);
          for (let i = 0; i < 65536; i++) h[i] = -10000 + (d[i * 4] * 65536 + d[i * 4 + 1] * 256 + d[i * 4 + 2]) * 0.1;
          st.dem.set(k, { z, x, y, h, contours: {} });
          if (st.dem.size > 400) for (const old of [...st.dem.keys()].slice(0, 100)) st.dem.delete(old);
          draw();
        }).catch(() => { if (st === state) { st.demPending.delete(k); st.dem.set(k, null); st.demNone.set(k, Date.now()) } });
    }
    return undefined;
  }
  function shade(t) {
    if (t.shade) return t.shade;
    const h = t.h, lat = y2lat(t.y + 0.5, t.z), m = 40075016 * Math.cos(lat * Math.PI / 180) / 2 ** t.z / 256;  // metres per pixel
    const ex = t.z < 7 ? 4 : t.z < 9 ? 2.5 : t.z < 11 ? 1.8 : t.z < 15 ? 1.3 : 2;  // gentle exaggeration so low-zoom relief reads; lidar's small banks and ditches need a little more
    const c = document.createElement('canvas'); c.width = c.height = 256; const g = c.getContext('2d'), img = g.createImageData(256, 256), o = img.data;
    const az = 315 * Math.PI / 180, alt = 45 * Math.PI / 180, lx = Math.sin(az) * Math.cos(alt), ly = -Math.cos(az) * Math.cos(alt), lz = Math.sin(alt);
    for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) {
      const i = y * 256 + x, l = h[y * 256 + Math.max(0, x - 1)], r = h[y * 256 + Math.min(255, x + 1)], u = h[Math.max(0, y - 1) * 256 + x], d = h[Math.min(255, y + 1) * 256 + x];
      const dx = (r - l) * ex / (2 * m), dy = (d - u) * ex / (2 * m), n = Math.hypot(dx, dy, 1);
      const hs = (-dx * lx + dy * ly + lz) / n, v = Math.max(0, Math.min(1, hs / lz));  // 1 on flat ground: multiply leaves it untouched
      const s = Math.round(255 * (0.55 + 0.45 * v));
      o[i * 4] = s; o[i * 4 + 1] = s; o[i * 4 + 2] = Math.min(255, s + 6); o[i * 4 + 3] = 255;
    }
    g.putImageData(img, 0, 0); return t.shade = c;
  }
  const CONTOUR = [[11, 50, 250], [12, 20, 100], [13, 10, 50], [15, 5, 25], [16, 2, 10]];  // [from zoom, interval m, index m]; 2 m only on lidar
  function contours(t, step) {
    if (t.contours[step]) return t.contours[step];
    const h = t.h, lines = [];  // marching squares on the 256x256 grid, in metres
    let lo = Infinity, hi = -Infinity; for (let i = 0; i < 65536; i++) { const v = h[i]; if (v < lo) lo = v; if (v > hi) hi = v }
    for (let lev = Math.ceil(lo / step) * step; lev <= hi; lev += step) {
      if (lev <= 0) continue;
      const seg = [];
      for (let y = 0; y < 255; y++) for (let x = 0; x < 255; x++) {
        const a = h[y * 256 + x], b = h[y * 256 + x + 1], c = h[(y + 1) * 256 + x + 1], d = h[(y + 1) * 256 + x];
        const idx = (a > lev) | ((b > lev) << 1) | ((c > lev) << 2) | ((d > lev) << 3);
        if (idx === 0 || idx === 15) continue;
        const e = [[x + (lev - a) / (b - a), y], [x + 1, y + (lev - b) / (c - b)], [x + (lev - d) / (c - d), y + 1], [x, y + (lev - a) / (d - a)]];
        const P = { 1: [3, 0], 2: [0, 1], 3: [3, 1], 4: [1, 2], 5: [3, 0, 1, 2], 6: [0, 2], 7: [3, 2], 8: [2, 3], 9: [0, 2], 10: [0, 1, 2, 3], 11: [1, 2], 12: [1, 3], 13: [0, 1], 14: [0, 3] }[idx];
        for (let k = 0; k < P.length; k += 2) { const p = e[P[k]], q = e[P[k + 1]]; seg.push(p[0] + .5, p[1] + .5, q[0] + .5, q[1] + .5) }
      }
      if (seg.length) lines.push({ lev, index: lev % CONTOUR.find(c => c[1] === step)[2] === 0, seg: new Float32Array(seg) });
    }
    return t.contours[step] = lines;
  }
  function terrainLayer(g, V, noRelief) {
    if (!st.style.relief && !st.style.contours) return;
    const tz = Math.max(TZ0, Math.min(tzMax(st.lat, st.lon), Math.floor(st.z + 0.01))), f = 2 ** (st.z - tz), ts = 256 * f;
    const cx = lon2x(st.lon, tz), cy = lat2y(st.lat, tz), w = V.w, h = V.h;
    const x0 = Math.floor(cx - w / 2 / ts), x1 = Math.floor(cx + w / 2 / ts), y0 = Math.max(0, Math.floor(cy - h / 2 / ts)), y1 = Math.min(2 ** tz - 1, Math.floor(cy + h / 2 / ts));
    const tiles = [];
    for (let ty = y0; ty <= y1; ty++) for (let tx = x0; tx <= x1; tx++) {
      const n = 2 ** tz, wx = ((tx % n) + n) % n, t = demTile(tz, wx, ty);
      tiles.push([tx, ty, t]);
    }
    if (st.style.relief && !noRelief) {
      g.save(); g.globalCompositeOperation = 'multiply'; g.imageSmoothingQuality = 'high';
      for (const [tx, ty, t] of tiles) {
        const sx = w / 2 + (tx - cx) * ts, sy = h / 2 + (ty - cy) * ts;
        if (t) g.drawImage(shade(t), sx, sy, ts + 0.5, ts + 0.5);
        else if (t === undefined || tz > TZN) {  // a coarser tile stands in while this one loads (or where lidar has no tile)
          if (t === null) { const wx = ((tx % 2 ** tz) + 2 ** tz) % 2 ** tz, k = 2 ** (tz - TZN); demTile(TZN, Math.floor(wx / k), Math.floor(ty / k)) }  // make sure the 30 m tile comes
          for (let up = 1; up <= 6 && tz - up >= TZ0; up++) {
            const k = 2 ** up, wx = ((tx % 2 ** tz) + 2 ** tz) % 2 ** tz, p = st.dem.get(tileKey(tz - up, Math.floor(wx / k), Math.floor(ty / k)));
            if (p) { const S = 256 / k; g.drawImage(shade(p), (wx - Math.floor(wx / k) * k) * S, (ty - Math.floor(ty / k) * k) * S, S, S, sx, sy, ts + 0.5, ts + 0.5); break }
          }
        }
      }
      g.restore();
    }
    const rule = [...CONTOUR].reverse().find(c => st.z >= c[0] && (c[0] < 16 || tz > TZN));
    if (st.style.contours && rule) {
      const [, step] = rule, k = ts / 256;
      g.save(); g.lineJoin = 'round';
      for (const index of [false, true]) {
        g.beginPath();
        for (const [tx, ty, t] of tiles) {
          if (!t) continue;
          const sx = w / 2 + (tx - cx) * ts, sy = h / 2 + (ty - cy) * ts;
          for (const L of contours(t, step)) if (L.index === index) { const a = L.seg; for (let i = 0; i < a.length; i += 4) { g.moveTo(sx + a[i] * k, sy + a[i + 1] * k); g.lineTo(sx + a[i + 2] * k, sy + a[i + 3] * k) } }
        }
        g.strokeStyle = index ? 'rgba(140,92,48,.75)' : 'rgba(150,104,60,.42)'; g.lineWidth = index ? 1.3 : 0.7; g.stroke();
      }
      // a few well-spaced height labels on the heavier index lines
      g.font = '600 10px -apple-system,Segoe UI,sans-serif'; g.textAlign = 'center'; g.textBaseline = 'middle';
      const placed = [];
      for (const [tx, ty, t] of tiles) {
        if (!t) continue; const sx = w / 2 + (tx - cx) * ts, sy = h / 2 + (ty - cy) * ts;
        for (const L of contours(t, step)) { if (!L.index) continue; const a = L.seg;
          for (let i = 0; i < a.length && placed.length < 24; i += 4 * 61) { const x = sx + a[i] * k, y = sy + a[i + 1] * k; if (x < 40 || x > w - 70 || y < 70 || y > h - 50) continue;
            if (placed.some(p => Math.abs(p[0] - x) < 130 && Math.abs(p[1] - y) < 70)) continue; placed.push([x, y]);
            const s = I18N.unit(L.lev, 'meter'); g.lineWidth = 3; g.strokeStyle = 'rgba(255,255,255,.85)'; g.strokeText(s, x, y); g.fillStyle = 'rgba(120,76,36,.95)'; g.fillText(s, x, y) } }
      }
      g.restore();
    }
  }
  // Aerial photos (/api/map/aerial/z/x/y, zoom 0-16, and finer only where the map set has them)
  function setPhoto(on) {
    st.style.aerial = on; save('mapStyle', st.style);  // tiles in the other style re-render as they are next needed (getTile)
    const b = $('map-photo'); if (b) { b.setAttribute('aria-pressed', on); b.classList.toggle('on', on) }
    const c = document.querySelector('[data-style=aerial]'); if (c) c.checked = on;
    draw();
  }
  // Cache entries: ImageBitmap, LOADING, or {none, at} for "no photo here (yet)". The photo download is still filling in,
  // so "none" is asked again after a minute at zoom 17+ (being downloaded now), 10 minutes below; a failed request after 10 s. Hits move to the end, so eviction is least-recent.
  const LOADING = 'loading';
  function aerialTile(z, x, y) {  // ImageBitmap, null (none there), or undefined (loading)
    const k = tileKey(z, x, y);
    if (st.aerial.has(k)) {
      const t = st.aerial.get(k);
      if (t === LOADING) return undefined;
      if (!t.none) { st.aerial.delete(k); st.aerial.set(k, t); return t }
      if (Date.now() - t.at < (t.failed ? 10000 : z >= 17 ? 60000 : 600000) || st.aerialPending >= 12) return null;
    } else if (st.aerialPending >= 12) return undefined;
    st.aerial.set(k, LOADING); st.aerialPending++;
    const state = st, cache = st.aerial;  // the cache is replaced when sharp photos are added (st.aerialRev): drop answers for the old one
    fetch(`/api/map/aerial/${k}${z >= 17 && st.aerialRev ? '?r=' + encodeURIComponent(st.aerialRev) : ''}`).then(r => { if (r.status !== 200 && r.status !== 204) throw Error(r.status); return r.status === 200 ? r.blob() : null })
      .then(b => b && createImageBitmap(b))
      .then(img => {
        if (st !== state || st.aerial !== cache) return img?.close?.();
        st.aerial.delete(k); st.aerial.set(k, img || { none: true, at: Date.now() });
        for (const [old, v] of st.aerial) { if (st.aerial.size <= 500) break; if (v === LOADING) continue; v.close?.(); st.aerial.delete(old) }
      })
      .catch(() => { if (st === state && st.aerial === cache) { st.aerial.delete(k); st.aerial.set(k, { none: true, failed: true, at: Date.now() }) } })
      .finally(() => { if (st === state) { st.aerialPending--; draw() } });
    return undefined;
  }
  function aerialLayer(g, V) {
    st.aerial = st.aerial || new Map(); st.aerialPending = st.aerialPending || 0;
    // zoom 17-19 photos exist only in the full-detail areas (/api/map/info aerial_detail = [w, s, e, n])
    const b = st.aerialDetail, detail = b && st.lon >= b[0] && st.lon <= b[2] && st.lat >= b[1] && st.lat <= b[3];
    // one zoom finer on high-density screens, so a photo pixel is about one screen pixel instead of two
    const tz = Math.max(0, Math.min(detail ? 19 : 16, Math.floor(st.z + Math.log2(Math.min(2, devicePixelRatio || 1)) + 0.01))), ts = 256 * 2 ** (st.z - tz), cx = lon2x(st.lon, tz), cy = lat2y(st.lat, tz), w = V.w, h = V.h;
    const x0 = Math.floor(cx - w / 2 / ts), x1 = Math.floor(cx + w / 2 / ts), y0 = Math.max(0, Math.floor(cy - h / 2 / ts)), y1 = Math.min(2 ** tz - 1, Math.floor(cy + h / 2 / ts));
    g.save(); g.imageSmoothingQuality = 'high';
    for (let ty = y0; ty <= y1; ty++) for (let tx = x0; tx <= x1; tx++) {
      const n = 2 ** tz, wx = ((tx % n) + n) % n, sx = w / 2 + (tx - cx) * ts, sy = h / 2 + (ty - cy) * ts;
      const t = aerialTile(tz, wx, ty);
      if (t) { g.drawImage(t, sx, sy, ts + 0.5, ts + 0.5); continue }
      // not there (or still loading): the nearest lower zoom that has a photo, enlarged
      for (let up = 1; up <= 8 && tz - up >= 0; up++) {
        const f = 2 ** up, pxx = Math.floor(wx / f), pyy = Math.floor(ty / f), pk = tileKey(tz - up, pxx, pyy);
        const pt = t === null ? aerialTile(tz - up, pxx, pyy) : (v => v && v !== LOADING && !v.none ? v : undefined)(st.aerial.get(pk));
        if (pt) { const S = 256 / f; g.drawImage(pt, (wx - pxx * f) * S, (ty - pyy * f) * S, S, S, sx, sy, ts + 0.5, ts + 0.5); break }
        if (pt === undefined && t === null) break;  // parent still loading
      }
    }
    g.restore();
  }
  // Orchard / vineyard colour overlay (/api/map/crops/z/x/y.png, zoom 7-13)
  function cropLayer(g, V) {
    if (st.z < 7) return;
    const tz = Math.min(13, Math.floor(st.z + 0.01)), ts = 256 * 2 ** (st.z - tz), cx = lon2x(st.lon, tz), cy = lat2y(st.lat, tz), w = V.w, h = V.h;
    const x0 = Math.floor(cx - w / 2 / ts), x1 = Math.floor(cx + w / 2 / ts), y0 = Math.max(0, Math.floor(cy - h / 2 / ts)), y1 = Math.min(2 ** tz - 1, Math.floor(cy + h / 2 / ts));
    st.crops = st.crops || new Map();
    g.save(); g.imageSmoothingEnabled = st.z < 13; g.globalAlpha = 0.75;
    for (let ty = y0; ty <= y1; ty++) for (let tx = x0; tx <= x1; tx++) {
      const k = tileKey(tz, tx, ty); let t = st.crops.get(k);
      if (t === undefined) {
        st.crops.set(k, null); const state = st;
        fetch(`/api/map/crops/${k}.png${st.ver ? '?v=' + st.ver : ''}`).then(r => r.status === 200 ? r.blob() : null).then(b => b && createImageBitmap(b))
          .then(img => { if (st === state && img) { st.crops.set(k, img); if (st.crops.size > 300) st.crops.delete(st.crops.keys().next().value); draw() } }).catch(() => { });
        continue;
      }
      if (t) g.drawImage(t, w / 2 + (tx - cx) * ts, h / 2 + (ty - cy) * ts, ts + 0.5, ts + 0.5);
    }
    g.restore();
  }
  // Height (m) at a point from the most detailed loaded tile, or null. With load=true, fetches the most detailed tile.
  function elevAt(lat, lon, load) {
    const top = tzMax(lat, lon);
    for (let z = top; z >= TZ0; z--) {
      const fx = lon2x(lon, z), fy = lat2y(lat, z), x = Math.floor(fx), y = Math.floor(fy), t = st.dem.get(tileKey(z, x, y));
      if (z === top && t === undefined && load) demTile(z, x, y);
      if (t === null && z === TZN) return null;  // no data here (ocean, or outside this map); a missing lidar tile falls through
      if (!t) continue;
      const px = Math.min(254.999, Math.max(0, (fx - x) * 256 - 0.5)), py = Math.min(254.999, Math.max(0, (fy - y) * 256 - 0.5)), i = Math.floor(px), j = Math.floor(py), a = px - i, b = py - j, H = t.h;
      return H[j * 256 + i] * (1 - a) * (1 - b) + H[j * 256 + i + 1] * a * (1 - b) + H[(j + 1) * 256 + i] * (1 - a) * b + H[(j + 1) * 256 + i + 1] * a * b;
    }
    return null;
  }
  async function elevation(lat, lon) {  // waits for the detailed tile
    for (const z of [...new Set([tzMax(lat, lon), TZN])]) {  // the lidar tile, and the 30 m one in case lidar has none here
      const x = Math.floor(lon2x(lon, z)), y = Math.floor(lat2y(lat, z));
      for (let i = 0; i < 40 && st && st.dem.get(tileKey(z, x, y)) === undefined; i++) { demTile(z, x, y); await new Promise(r => setTimeout(r, 100)) }
    }
    return st ? elevAt(lat, lon) : null;
  }
  const fmtElev = m => I18N.unit(Math.round(m), 'meter');   // heights in metres only

  // ------------------------------------------------------------------ card ----
  function openCard(html) {
    const el = $('card'); el.innerHTML = `<button class="x" aria-label="${th('map.card.close')}">×</button>` + html; el.hidden = false; el.scrollTop = 0;
    $('layers').hidden = true; $('layers-btn').setAttribute('aria-expanded', 'false'); $('pres').hidden = true;  // one floating panel at a time
    el.querySelector('.x').onclick = closeCard; return el;
  }
  // Keep the chosen place visible beside (desktop) or above (phone) the card instead of underneath it.
  function reveal(lat, lon) {
    const card = $('card'), cv = st.cv; if (!card || card.hidden) return;
    const w = cv.clientWidth, h = cv.clientHeight, c = card.getBoundingClientRect(), m = cv.getBoundingClientRect();
    const x = w / 2 + (lon2x(lon, st.z) - lon2x(st.lon, st.z)) * 256, y = h / 2 + (lat2y(lat, st.z) - lat2y(st.lat, st.z)) * 256;
    const sheet = c.width > w * 0.8;  // phone bottom sheet
    // the card may still grow (lists load after it opens), so reserve its largest size
    const cl = c.left - m.left, cr = c.right - m.left, ct = sheet ? h * 0.42 : c.top - m.top, cb = sheet ? h : h - 60;
    const covered = x > cl - 24 && x < cr + 24 && y > ct - 24 && y < cb + 24;
    if (!covered && x > 20 && x < w - 70 && y > 60 && y < h - 30) return;
    const tx = sheet ? w / 2 : (cr + 24 + w - 60) / 2, ty = sheet ? Math.max(70, ct / 2 + 20) : h / 2;
    pan(tx - x, ty - y); st.listAt = 1; settle(true);  // the nearby list stays put while the map makes room
  }
  function closeCard() { const el = $('card'); if (el) el.hidden = true; if (st) { st.sel = null; draw() } document.querySelectorAll('#near button.on').forEach(b => b.classList.remove('on')) }
  function showPlace(p) { openCard(`<div class="kind"><i class="msym" style="background:#1565c0">◉</i>${escH(p.kind || t('map.place'))}</div><h3>${escH(p.name)}</h3>${p.detail ? `<p>${escH(p.detail)}</p>` : ''}${coordsBlock(p)}<div class="acts"><button data-a="clear">${th('map.card.remove_marker')}</button></div>`).querySelector('[data-a=clear]').onclick = () => { st.mark = null; closeCard() }; wireCoords(p) }
  function coordsBlock(p) {
    const home = load('mapHome', HOME), km = home && kmBetween(home.lat, home.lon, p.lat, p.lon);
    const id = 'el' + Math.random().toString(36).slice(2, 8);
    elevation(p.lat, p.lon).then(m => { const el = document.getElementById(id); if (el) el.textContent = m == null ? '' : t('map.card.elevation', { elev: fmtElev(m) }) });
    return `<p class="coords">${p.lat.toFixed(5)}, ${p.lon.toFixed(5)}<br>${home ? `<span class="muted">${th('map.card.from_area', { dist: dist(km) })}</span><br>` : ''}<span class="muted" id="${id}">${th('map.card.elevation_loading')}</span></p>`;
  }
  function wireCoords(p) {
    const el = $('card');
    el.querySelector('[data-a=copy]')?.addEventListener('click', async e => { try { await navigator.clipboard.writeText(`${p.lat.toFixed(5)}, ${p.lon.toFixed(5)}`); e.target.textContent = t('map.card.copied') + ' ✓' } catch { e.target.textContent = t('map.card.copy_failed') } });
    el.querySelector('[data-a=zoom]')?.addEventListener('click', () => { go(p.lat, p.lon, Math.max(st.z, 16)); reveal(p.lat, p.lon) });
  }
  function showDetail(p, back) {
    const l = layerOf(p.cls), detail = p.detail || {};
    st.sel = { lat: p.lat, lon: p.lon, cls: p.cls }; draw();
    const rows = Object.entries(detail).filter(([k, v]) => FIELD[k] && v != null && String(v).trim() !== '' && !(k === 'commod2' && v === detail.commod1))
      .map(([k, v]) => `<tr><th>${th(FIELD[k])}</th><td>${escH(fieldValue(k, v))}</td></tr>`).join('');
    const notWater = detail.not_drinking_water ? `<p class="caveat"><b>${th('map.caveat.not_water_head')}</b> ${th('map.caveat.not_water', { kind: detail.reservoir_type ? String(detail.reservoir_type).toLowerCase() : t('map.caveat.waste_pond') })}</p>` : '';
    const tailings = /tailings/i.test(detail.purposes || '') ? `<p class="caveat"><b>${th('map.caveat.tailings_head')}</b> ${th('map.caveat.tailings')}</p>` : '';
    const summary = summaryOf(p.summary_parts, p.summary);
    const el = openCard(`${back ? `<button class="back">← ${escH(back.title)}</button>` : ''}<div class="kind">${sym(l, 20)}${escH(lname(l))}</div><h3>${escH(unnamed(p))}</h3>
      ${p.records > 1 ? `<p class="muted small">${th('map.card.source_records', { n: p.records })}</p>` : ''}
      ${rows ? `<table>${rows}</table>` : summary ? `<p>${escH(summary)}</p>` : ''}${notWater}${tailings}${CAVEAT[p.cls] && !detail.not_drinking_water ? `<p class="caveat">${th(CAVEAT[p.cls])}</p>` : ''}${coordsBlock(p)}
      <div class="acts"><button data-a="zoom">${th('map.card.zoom_here')}</button><button data-a="copy">${th('map.card.copy')}</button></div>`);   // no web links: the words stay, the addresses go
    if (back) el.querySelector('.back').onclick = () => showMembers(back.p);
    wireCoords(p); reveal(p.lat, p.lon);
  }
  async function showMembers(p) {
    const serial = ++st.serial.members, state = st, b = p.bbox || [p.lon, p.lat, p.lon, p.lat], pad = 1e-5;
    st.sel = { lat: p.lat, lon: p.lon }; draw();
    const title = p.spot ? t('map.n_records_spot', { n: Math.max(p.count, p.records || 0) }) : t('map.n_places_here', { n: p.count });
    const kind = p.spot ? escH(lname(layerOf(p.cls))) : th('map.card.group');
    openCard(`<div class="kind">${sym(layerOf(p.cls), 20)}${kind}</div><h3>${escH(title)}</h3><p class="muted">${th('map.loading')}</p>`);
    reveal(p.lat, p.lon);
    try {
      const q = new URLSearchParams({ west: b[0] - pad, south: b[1] - pad, east: b[2] + pad, north: b[3] + pad, lat: p.lat, lon: p.lon, kinds: Object.keys(p.kinds || { [p.cls]: 1 }).join(',') });
      const r = await fetch('/api/map/members?' + q).then(r => r.json());
      if (st !== state || serial !== st.serial.members) return;
      const items = r.items || [], names = new Set(items.map(m => m.name));
      if (items.length === 1) return showDetail(items[0]);
      const head = p.spot && names.size === 1 && items[0]?.name ? items[0].name : title;
      const el = openCard(`<div class="kind">${sym(layerOf(p.cls), 20)}${kind}</div><h3>${escH(head)}</h3>${breakdown(p)}
        <p class="muted small">${r.total > items.length ? th('map.card.showing_nearest', { n: items.length, total: I18N.num(r.total) + (r.capped ? '+' : '') }) : th('map.card.n_records', { n: items.length })}</p>
        <div class="members">${items.map((m, i) => `<button data-i="${i}">${sym(layerOf(m.cls), 20)}<span><b>${escH(unnamed(m))}</b><small>${escH([summaryOf(m.summary_parts, m.summary) || lname(layerOf(m.cls)), m.detail?.WeekOfPlantStart ? t('map.card.stocked', { week: String(m.detail.WeekOfPlantStart) }) : ''].filter(Boolean).join(' · '))}</small></span></button>`).join('')}</div>
        <div class="acts"><button data-a="zoom">${th('map.card.zoom_here')}</button></div>`);
      el.querySelectorAll('.members button').forEach(bt => bt.onclick = () => showDetail(items[+bt.dataset.i], { title, p }));
      el.querySelector('[data-a=zoom]').onclick = () => fit(b);
    } catch { if (st === state) openCard(`<h3>${escH(title)}</h3><p>${th('map.card.list_failed')}</p>`) }
  }

  // ------------------------------------------------------------------ data ----
  async function loadPins() {
    if (!st) return;
    const state = st, cv = st.cv, ks = kinds(), z = Math.floor(st.z + 0.05);  // never a finer grid than the screen shows
    if (!ks.length) { st.pins = []; st.pinBox = null; statusLine(st.have && !st.have.size ? '' : t('map.status.no_layers')); renderLayers(); draw(); return }
    const cx = lon2x(st.lon, st.z), cy = lat2y(st.lat, st.z), dx = cv.clientWidth / 256 * 0.75, dy = cv.clientHeight / 256 * 0.75;  // view + 25% margin each side
    const box = [x2lon(cx - dx, st.z), y2lat(cy + dy, st.z), x2lon(cx + dx, st.z), y2lat(cy - dy, st.z)];
    const v = [x2lon(cx - dx / 1.5, st.z), y2lat(cy + dy / 1.5, st.z), x2lon(cx + dx / 1.5, st.z), y2lat(cy - dy / 1.5, st.z)];
    const key = ks.join(',') + '|' + z;
    if (st.pinBox && st.pinKey === key && v[0] >= st.pinBox[0] && v[1] >= st.pinBox[1] && v[2] <= st.pinBox[2] && v[3] <= st.pinBox[3]) return countLine();
    const serial = ++st.serial.pins;
    statusLine(t('map.status.loading'));
    try {
      const q = new URLSearchParams({ west: box[0], south: box[1], east: box[2], north: box[3], zoom: z, kinds: ks.join(',') });
      const data = await fetch('/api/map/pins?' + q).then(r => r.json());
      if (st !== state || serial !== st.serial.pins) return;
      if (!data.points) throw Error(data.error || 'no points');
      st.pins = data.points; st.pinBox = box; st.pinKey = key; st.truncated = data.truncated;
      countLine(); renderLayers(); draw();
    } catch (e) { if (st === state && serial === st.serial.pins) statusLine(t('map.status.failed')) }
  }
  function inView(p) {
    const w = st.cv.clientWidth, h = st.cv.clientHeight, x = w / 2 + (lon2x(p.lon, st.z) - lon2x(st.lon, st.z)) * 256, y = h / 2 + (lat2y(p.lat, st.z) - lat2y(st.lat, st.z)) * 256;
    return x >= 0 && x <= w && y >= 0 && y <= h;
  }
  function countLine() {
    let n = 0; for (const p of st.pins) if (inView(p)) n += p.count;
    renderLayers();
    statusLine(n ? t(st.truncated ? 'map.status.in_view_busiest' : 'map.status.in_view', { n }) : t('map.status.nothing_in_view'));
  }
  async function nearby() {
    if (!st) return;
    const box = $('near'), state = st, serial = ++st.serial.near, ks = kinds(); if (!box) return;
    if (!ks.length) { box.innerHTML = `<p class="muted small">${th('map.near.no_layers')}</p>`; return }
    box.classList.add('busy');
    try {
      const r = await fetch(`/api/map/near?lat=${st.lat}&lon=${st.lon}&cls=${ks.join(',')}&limit=${Math.max(3, Math.ceil(14 / ks.length))}`).then(r => r.json());
      if (st !== state || serial !== st.serial.near) return;
      box.classList.remove('busy');
      if (!Array.isArray(r) || !r.length) { box.innerHTML = `<p class="muted small">${th('map.near.nothing')}</p>`; return }
      const list = r.sort((a, b) => a.km - b.km).slice(0, 14);
      box.innerHTML = list.map((p, i) => { const line = summaryOf(p.detail_parts, p.detail); return `<button data-i="${i}">${sym(layerOf(p.cls), 24)}<span><b>${escH(unnamed(p))}</b><small>${th('map.near.line', { kind: lname(layerOf(p.cls)), dist: dist(p.km), dir: DIR[p.dir] ? t(DIR[p.dir]) : p.dir || '' })}</small>${line ? `<small>${escH(line)}</small>` : ''}</span></button>` }).join('');
      box.querySelectorAll('button').forEach(b => b.onclick = () => {
        const p = list[+b.dataset.i]; box.querySelectorAll('button.on').forEach(x => x.classList.remove('on')); b.classList.add('on');
        st.z = Math.max(st.z, 16); st.lat = p.lat; st.lon = p.lon; st.listAt = 1; showDetail({ ...p, summary: p.detail, summary_parts: p.detail_parts, detail: {} }); settle(true); b.classList.add('on');
        if (innerWidth < 1000) $('mapwrap').scrollIntoView({ behavior: 'smooth', block: 'start' });
      });
    } catch { if (st === state) box.innerHTML = `<p class="muted small">${th('map.near.unavailable')}</p>` }
  }
  async function findPlace(q) {
    const box = $('pres'); if (!q) { box.hidden = true; return }
    $('layers').hidden = true;
    const state = st, serial = ++st.serial.place;
    box.hidden = false; $('card').hidden = true; box.innerHTML = `<p class="muted small">${th('map.find.searching')}</p>`;
    try {
      const res = await fetch('/api/places?q=' + encodeURIComponent(q) + `&near=${st.lat.toFixed(3)},${st.lon.toFixed(3)}`), r = await res.json();
      if (st !== state || serial !== st.serial.place) return;
      if (!r.length) {
        const address = res.headers.get('X-Query-Kind') === 'address';   // the engine says how it read the query: address, postcode or place
        const offer = IN_WINDOW && !addressesHere();   // address search for the region is offered only where it is not there yet
        box.innerHTML = `<p class="muted small" style="padding:8px">${address ? th('map.find.no_address') + (offer ? ` <a href="/ui/setup.html#manage">${th('map.find.add_addresses')}</a>.` : '') : th('map.find.no_place')}</p>`;
        return;
      }
      // the marker keeps the kind and the line under the name as shown; the zoom goes by the engine's own kind
      const pick = p => { box.hidden = true; st.mark = { lat: p.lat, lon: p.lon, name: p.name, kind: kindName(p), detail: placeDetail(p) }; closeCard(); go(p.lat, p.lon, /county/i.test(p.kind) ? 10 : /postcode|postal|city|town|populated|^(Lake|Reservoir)$/i.test(p.kind) ? 13 : 15); st.cv.focus() };
      box.innerHTML = `<div class="small muted" style="padding:4px 8px">${th('map.find.matches', { n: r.length })}</div>` + r.map((p, i) => `<button data-i="${i}"><b>${escH(p.name)}</b><small>${escH([kindName(p), placeDetail(p)].filter(Boolean).join(' · '))}</small></button>`).join('');
      box.querySelectorAll('button').forEach(b => b.onclick = () => pick(r[+b.dataset.i]));
      if (r.length === 1) pick(r[0]);
    } catch { if (st === state) box.innerHTML = `<p class="muted small">${th('map.find.unavailable')}</p>` }
  }
  // The map's places as this page says them, for other pages (web/chat.js, the chat's map sources): a direction as the engine sends
  // it ("south-east"), a place's kind, and the line under a place's name
  const dirName = d => typeof d === 'string' && Object.hasOwn(DIR, d) ? t(DIR[d]) : String(d || '');
  return { page, stop, dirName, kindName, placeDetail };
})();
