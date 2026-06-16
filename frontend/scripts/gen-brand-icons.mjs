// Generates src/workflow-ui/brand-icons.generated.ts: a base-name -> BrandIcon
// map of FULL-COLOUR connector logos used in the palette / node cards / quick-add.
//
// Two colour sources (build-time only; only the resolved markup is inlined, so
// the app bundle carries no icon-library dependency):
//   1. gilbarbara/logos (svgporn), CC0 - true multi-colour original logos, fetched
//      from jsdelivr. Stored as { svg } and rendered as an <img> data-URI.
//   2. simple-icons (+ legacy v9 for trademark-removed enterprise marks) - a
//      single-path mark tinted with the brand's official colour, for brands
//      gilbarbara doesn't carry. Stored as { path, color }.
// Anything in neither falls back to a generic lucide icon at render time.
//
// Run: node scripts/gen-brand-icons.mjs   (needs network for the gilbarbara CDN)
import * as si from 'simple-icons';
import * as siLegacy from 'si-legacy';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dir = dirname(fileURLToPath(import.meta.url));
const GH = 'https://cdn.jsdelivr.net/gh/gilbarbara/logos@main/logos';

// base name -> gilbarbara/logos slug (full multi-colour). Prefer the square
// "-icon" variant where one exists so marks sit evenly in the list.
const GB = {
    postgres: 'postgresql',
    pgvector: 'postgresql',
    mysql: 'mysql',
    mariadb: 'mariadb',
    oracle: 'oracle',
    db2: 'ibm',
    sqlite: 'sqlite',
    snowflake: 'snowflake-icon',
    redshift: 'aws-redshift',
    synapse: 'microsoft-azure',
    azureblob: 'microsoft-azure',
    eventhubs: 'microsoft-azure',
    s3: 'aws-s3',
    gcs: 'google-cloud',
    pubsub: 'google-cloud',
    r2: 'cloudflare',
    kafka: 'kafka-icon',
    nats: 'nats',
    rabbit: 'rabbitmq-icon',
    kinesis: 'aws-kinesis',
    dynamodb: 'aws-dynamodb',
    mongodb: 'mongodb-icon',
    cassandra: 'cassandra',
    redis: 'redis',
    elastic: 'elasticsearch',
    opensearch: 'opensearch',
    couchdb: 'couchdb',
    qdrant: 'qdrant',
    milvus: 'milvus',
    pinecone: 'pinecone',
    chroma: 'chroma',
    orc: 'apache',
    graphql: 'graphql',
    dbt: 'dbt',
    git: 'git-icon',
    github: 'github-icon',
    gitlab: 'gitlab',
    salesforce: 'salesforce',
    hubspot: 'hubspot',
    zendesk: 'zendesk',
    intercom: 'intercom',
    stripe: 'stripe',
    xero: 'xero',
    shopify: 'shopify',
    notion: 'notion',
    airtable: 'airtable',
    asana: 'asana',
    trello: 'trello',
    monday: 'monday',
    linear: 'linear',
    jira: 'jira',
    mailchimp: 'mailchimp',
    sendgrid: 'sendgrid',
    segment: 'segment',
    slack: 'slack-icon',
    discord: 'discord-icon',
    telegram: 'telegram',
    twilio: 'twilio',
    // pipedrive: only a wide wordmark exists (no square mark in either source),
    // so it falls back to a generic lucide icon rather than a tiny strip.
};

// base name -> simple-icons slug (single mark, tinted with brand colour), for
// brands gilbarbara doesn't carry.
const SI = {
    sqlserver: 'microsoftsqlserver',
    bigquery: 'googlebigquery',
    excel: 'microsoftexcel',
    'excel-online': 'microsoftexcel',
    gsheets: 'googlesheets',
    databricks: 'databricks',
    clickhouse: 'clickhouse',
    cockroach: 'cockroachlabs',
    pulsar: 'apachepulsar',
    duckdb: 'duckdb',
    ducklake: 'duckdb',
    quack: 'duckdb',
    minio: 'minio',
    b2: 'backblaze',
    scylla: 'scylladb',
    avro: 'apacheavro',
    parquet: 'apacheparquet',
    delta: 'databricks',
    spatial: 'geopandas',
    quickbooks: 'quickbooks',
    clickup: 'clickup',
};

// Custom raw-SVG logos for brands neither gilbarbara nor simple-icons
// carry (fetched from the vendor and embedded). Applied last, with
// precedence over the GB/SI results.
const CUSTOM = {
    motherduck: { svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 176.758 159.821\"><path d=\"M5.405,26.223c-.149-.68-.306-1.343-.454-2.014a38.12,38.12,0,0,0,9.686-.888c8.932-1.749,9.015-4.821,9.015-4.821,1.12-4.34,4.289-2.739,4.289-2.739,3.475,2.136,1.461,6.2,1.461,6.2C24.454,32.015,16.223,38.5,9.5,42.4c4.141.024,9.838,1.256,18.766,6.246a2.034,2.034,0,0,1,.873,2.055c-.143.725-1.069,1.019-2.21.734,0,0-8.9-3.517-17.1-1.408A28.252,28.252,0,0,1,0,50.733c4.954-7.069,7.292-15.5,5.406-24.511\" transform=\"translate(146.776 30.486)\" fill=\"#f4783b\"/><path d=\"M8.457,109.882s.2-6.662,3.671-17.4c0,0,7.4-17.412,13.47-17.225,0,0,2.147.065,3.778,4.429,0,0,2.156,6.535,16.631,7.372,0,0,21.879,1.972,37.814,3.448A46.247,46.247,0,0,1,67.607,53.848c.523-16.933,10.253-33.175,24.077-40.859a4.555,4.555,0,0,1,.535-1.1C96.784,4.58,104.854.291,104.854.291c5.213-1.31,2.792,2.219,2.792,2.219-2.537,2.516-.422,4.773,1.016,5.866a34.787,34.787,0,0,1,6.686.591c22.063,4.123,38.848,18.894,44.89,47.726,3.3,15.763-6.353,29.979-20.317,37.782,0,0-9.95,6.6-1.292,18.555,0,0,11.2,17.168,3.98,28.449,0,0-7.446,19.892-60.7,18.243,0,0-80.185,3.181-73.455-49.841\" transform=\"translate(-8.056 0)\" fill=\"#fff200\"/><path d=\"M15.188,20.907c-.642-3.282.039-6.178,1.524-6.469s3.2,2.139,3.846,5.424-.048,6.178-1.53,6.469-3.2-2.142-3.841-5.424\" transform=\"translate(111.568 28.403)\" fill=\"#000000\"/></svg>", title: 'MotherDuck' },
};

// --- simple-icons index (legacy first, current overwrites) ---
const bySlug = new Map();
for (const v of Object.values(siLegacy)) {
    if (v && typeof v === 'object' && v.slug && v.path) bySlug.set(v.slug, v);
}
for (const v of Object.values(si)) {
    if (v && typeof v === 'object' && v.slug && v.path) bySlug.set(v.slug, v);
}

// simple-icons slug overrides for the wide-wordmark fallback (where the slug
// isn't just the base name).
const SI_FALLBACK = {
    db2: 'ibm',
    cassandra: 'apachecassandra',
    couchdb: 'apachecouchdb',
    nats: 'natsdotio',
    rabbit: 'rabbitmq',
    kafka: 'apachekafka',
    orc: 'apache',
};
const siFor = base => bySlug.get(SI_FALLBACK[base] || base);

// Trim an svgporn SVG down to just its <svg>...</svg> markup.
function cleanSvg(s) {
    const i = s.indexOf('<svg');
    const j = s.lastIndexOf('</svg>');
    if (i < 0 || j < 0) return null;
    return s.slice(i, j + 6).replace(/\r?\n\s*/g, ' ').replace(/<!--.*?-->/g, '').trim();
}

// Aspect ratio (w/h) of an SVG's viewBox; null if unknown.
function ratioOf(svg) {
    const m = svg.match(/viewBox="([\d.\- ]+)"/);
    if (!m) return null;
    const p = m[1].trim().split(/\s+/).map(Number);
    return p[2] && p[3] ? p[2] / p[3] : null;
}

const out = {};
const missing = [];

// Discover which gilbarbara slugs exist so we can prefer the square "-icon"
// logomark variant over a wide wordmark.
const flat = await (
    await fetch('https://data.jsdelivr.com/v1/packages/gh/gilbarbara/logos@main?structure=flat')
).json();
const available = new Set(
    flat.files
        .map(f => f.name)
        .filter(n => /^\/logos\/.*\.svg$/.test(n))
        .map(n => n.replace('/logos/', '').replace('.svg', '')),
);
function squareSlug(slug) {
    const stem = slug.replace(/-icon$/, '');
    for (const c of [`${stem}-icon`, slug]) if (available.has(c)) return c;
    return available.has(slug) ? slug : null;
}

// 1. gilbarbara multi-colour, preferring the square logomark. A logo that is
// still very wide/tall after that (e.g. a text-only wordmark) reads as a tiny
// strip in a square slot, so fall back to the square 24x24 simple-icons mark
// tinted with the brand colour.
const fetched = await Promise.all(
    Object.entries(GB).map(async ([base, slug]) => {
        const pick = squareSlug(slug);
        if (!pick) return [base, slug, null];
        try {
            const r = await fetch(`${GH}/${pick}.svg`);
            return [base, pick, r.ok ? cleanSvg(await r.text()) : null];
        } catch {
            return [base, pick, null];
        }
    }),
);
for (const [base, slug, svg] of fetched) {
    const r = svg ? ratioOf(svg) : null;
    const squareEnough = r !== null && r >= 0.45 && r <= 2.0;
    if (svg && squareEnough) {
        out[base] = { svg, title: slug };
        continue;
    }
    const icon = siFor(base); // square 24x24 fallback for wide/missing marks
    if (icon) out[base] = { path: icon.path, color: '#' + icon.hex, title: icon.title };
    else if (svg) out[base] = { svg, title: slug }; // wide, but better than nothing
    else missing.push(`gilbarbara ${base} -> ${slug}`);
}

// 2. simple-icons tinted fallback for brands gilbarbara doesn't carry at all.
for (const [base, slug] of Object.entries(SI)) {
    if (out[base]) continue;
    const icon = bySlug.get(slug);
    if (icon) out[base] = { path: icon.path, color: '#' + icon.hex, title: icon.title };
    else missing.push(`simple-icons ${base} -> ${slug}`);
}

for (const [base, v] of Object.entries(CUSTOM)) out[base] = v;

const header =
    '// AUTO-GENERATED by scripts/gen-brand-icons.mjs. Do not edit by hand.\n' +
    '// Full-colour connector logos. { svg } = gilbarbara/logos (rendered as an\n' +
    '// <img>); { path, color } = a simple-icons mark tinted with its brand colour.\n\n' +
    'export type BrandIcon =\n' +
    '    | { svg: string; title: string }\n' +
    '    | { path: string; color: string; title: string };\n\n' +
    'export const BRAND_ICONS: Record<string, BrandIcon> = ';
writeFileSync(
    join(__dir, '..', 'src', 'workflow-ui', 'brand-icons.generated.ts'),
    header + JSON.stringify(out, null, 2) + ';\n',
);

const svgCount = Object.values(out).filter(v => 'svg' in v).length;
const tintCount = Object.values(out).filter(v => 'path' in v).length;
console.log(`brand-icons: ${Object.keys(out).length} icons (${svgCount} colour SVG, ${tintCount} tinted)`);
if (missing.length) {
    console.log(`MISSING (${missing.length}) -> generic fallback:`);
    for (const m of missing) console.log('  ' + m);
}
