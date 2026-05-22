import { isTauri } from './tauri-dialog';

const WORKSPACE_PATH_KEY = 'duckle:workspace-path';

// Workspace v1 (single file, everything in one blob). Kept for the
// migration path.
const V1_FILE = 'workspace.json';
// Workspace v2 (this commit).
const METADATA_FILE = 'duckle.json';
const REPOSITORY_FILE = 'repository.json';
const PIPELINES_DIR = 'pipelines';
const CONNECTIONS_DIR = 'connections';
const CONTEXTS_DIR = 'contexts';
const ROUTINES_DIR = 'routines';
const DOCS_DIR = 'docs';

const PAYLOAD_DIR_BY_TYPE: Record<string, string> = {
    pipeline: PIPELINES_DIR,
    connection: CONNECTIONS_DIR,
    context: CONTEXTS_DIR,
    routine: ROUTINES_DIR,
    doc: DOCS_DIR,
};

export type WorkspaceState = {
    version: number;
    engine?: string;
    pipelineData?: Record<string, unknown>;
    repo?: unknown[];
    jobs?: unknown[];
    activeJobId?: string;
};

export function isInTauri(): boolean {
    return isTauri();
}

export function getWorkspacePath(): string | null {
    try {
        return localStorage.getItem(WORKSPACE_PATH_KEY);
    } catch {
        return null;
    }
}

export function setWorkspacePath(path: string): void {
    try {
        localStorage.setItem(WORKSPACE_PATH_KEY, path);
    } catch {
        /* ignore */
    }
}

export function clearWorkspacePath(): void {
    try {
        localStorage.removeItem(WORKSPACE_PATH_KEY);
    } catch {
        /* ignore */
    }
}

function joinPath(dir: string, ...parts: string[]): string {
    const sep = dir.includes('\\') && !dir.includes('/') ? '\\' : '/';
    return [dir.replace(/[/\\]+$/, ''), ...parts].join(sep);
}

export async function pickWorkspaceDirectory(): Promise<string | null> {
    if (!isTauri()) return null;
    try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const result = await open({
            directory: true,
            multiple: false,
            title: 'Choose Duckle workspace folder',
        });
        return typeof result === 'string' ? result : null;
    } catch (err) {
        console.error('Workspace picker failed', err);
        return null;
    }
}

type FsLib = typeof import('@tauri-apps/plugin-fs');

async function fs(): Promise<FsLib> {
    return await import('@tauri-apps/plugin-fs');
}

async function ensureDir(path: string): Promise<void> {
    const { exists, mkdir } = await fs();
    if (!(await exists(path))) {
        await mkdir(path, { recursive: true });
    }
}

async function writeJson(path: string, value: unknown): Promise<void> {
    const { writeTextFile } = await fs();
    await writeTextFile(path, JSON.stringify(value, null, 2));
}

async function readJsonIfExists<T = unknown>(path: string): Promise<T | null> {
    const { exists, readTextFile } = await fs();
    if (!(await exists(path))) return null;
    const content = await readTextFile(path);
    return JSON.parse(content) as T;
}

async function readDirEntries(path: string): Promise<string[]> {
    try {
        const { exists, readDir } = await fs();
        if (!(await exists(path))) return [];
        const entries = await readDir(path);
        return entries
            .filter(e => e.isFile && e.name.endsWith('.json'))
            .map(e => e.name);
    } catch {
        return [];
    }
}

// ---- Load (with migration) ---------------------------------------------

/**
 * Load the workspace from disk. Reads the v2 multi-file layout if it
 * exists; otherwise tries to migrate a v1 workspace.json on the fly.
 * Returns `null` only if there's nothing to load (fresh workspace) or
 * we're running in browser mode.
 */
export async function loadWorkspace(path: string): Promise<WorkspaceState | null> {
    if (!isTauri()) return null;
    try {
        const v2 = await loadV2(path);
        if (v2) return v2;
        const v1 = await loadAndMigrateV1(path);
        if (v1) return v1;
        return null;
    } catch (err) {
        console.error('Failed to load workspace', err);
        return null;
    }
}

async function loadV2(path: string): Promise<WorkspaceState | null> {
    const meta = await readJsonIfExists<{
        version?: number;
        engine?: string;
        jobs?: unknown[];
        activeJobId?: string;
    }>(joinPath(path, METADATA_FILE));
    if (!meta) return null;

    const repo = (await readJsonIfExists<Array<Record<string, unknown>>>(
        joinPath(path, REPOSITORY_FILE),
    )) ?? [];

    // Hydrate payloads for each repo item that lives in its own file.
    for (const item of repo) {
        const itype = typeof item.type === 'string' ? item.type : '';
        const dir = PAYLOAD_DIR_BY_TYPE[itype];
        if (!dir || itype === 'pipeline' || itype === 'folder' || itype === 'project') continue;
        const file = joinPath(path, dir, `${item.id}.json`);
        const payload = await readJsonIfExists(file);
        if (payload !== null) {
            (item as { payload: unknown }).payload = payload;
        }
    }

    // Load each pipeline file referenced in the repo.
    const pipelineData: Record<string, unknown> = {};
    for (const item of repo) {
        if (item.type !== 'pipeline') continue;
        const file = joinPath(path, PIPELINES_DIR, `${item.id}.json`);
        const pipeline = await readJsonIfExists(file);
        if (pipeline) pipelineData[item.id as string] = pipeline;
    }

    return {
        version: meta.version ?? 2,
        engine: meta.engine,
        jobs: meta.jobs,
        activeJobId: meta.activeJobId,
        repo,
        pipelineData,
    };
}

async function loadAndMigrateV1(path: string): Promise<WorkspaceState | null> {
    const v1Path = joinPath(path, V1_FILE);
    const v1 = await readJsonIfExists<WorkspaceState>(v1Path);
    if (!v1) return null;
    // Write v2 files alongside; archive v1.
    try {
        await saveAll(path, v1);
        const { writeTextFile, exists, remove } = await fs();
        const backup = joinPath(path, `${V1_FILE}.v1.bak`);
        await writeTextFile(backup, JSON.stringify(v1, null, 2));
        if (await exists(v1Path)) {
            try {
                await remove(v1Path);
            } catch {
                /* leave it if we can't remove */
            }
        }
        console.info('Migrated workspace from v1 -> v2');
    } catch (err) {
        console.warn('Migration failed; loading v1 in-memory only', err);
    }
    return v1;
}

// ---- Save (granular) ---------------------------------------------------

/**
 * Write the metadata file only - cheap; safe to call on every change.
 */
export async function saveMetadata(
    path: string,
    metadata: { engine?: string; jobs?: unknown; activeJobId?: string },
): Promise<void> {
    if (!isTauri()) return;
    try {
        await ensureDir(path);
        await writeJson(joinPath(path, METADATA_FILE), {
            version: 2,
            ...metadata,
        });
    } catch (err) {
        console.error('saveMetadata failed', err);
    }
}

/**
 * Write the repository tree (id, name, type, parentId, icon). Payloads
 * live in their own per-type directories.
 */
export async function saveRepository(
    path: string,
    items: Array<Record<string, unknown>>,
): Promise<void> {
    if (!isTauri()) return;
    try {
        await ensureDir(path);
        const stripped = items.map(i => {
            const { payload, ...rest } = i as Record<string, unknown> & { payload?: unknown };
            void payload;
            return rest;
        });
        await writeJson(joinPath(path, REPOSITORY_FILE), stripped);
    } catch (err) {
        console.error('saveRepository failed', err);
    }
}

export async function savePipelineFile(
    path: string,
    pipelineId: string,
    pipeline: unknown,
): Promise<void> {
    if (!isTauri()) return;
    try {
        const dir = joinPath(path, PIPELINES_DIR);
        await ensureDir(dir);
        await writeJson(joinPath(dir, `${pipelineId}.json`), pipeline);
    } catch (err) {
        console.error('savePipelineFile failed', err);
    }
}

export async function saveItemPayload(
    path: string,
    itemType: string,
    itemId: string,
    payload: unknown,
): Promise<void> {
    if (!isTauri()) return;
    const dir = PAYLOAD_DIR_BY_TYPE[itemType];
    if (!dir) return;
    try {
        const folder = joinPath(path, dir);
        await ensureDir(folder);
        await writeJson(joinPath(folder, `${itemId}.json`), payload);
    } catch (err) {
        console.error('saveItemPayload failed', err);
    }
}

export async function deletePipelineFile(
    path: string,
    pipelineId: string,
): Promise<void> {
    if (!isTauri()) return;
    try {
        const { exists, remove } = await fs();
        const file = joinPath(path, PIPELINES_DIR, `${pipelineId}.json`);
        if (await exists(file)) await remove(file);
    } catch (err) {
        console.warn('deletePipelineFile failed', err);
    }
}

export async function deleteItemPayload(
    path: string,
    itemType: string,
    itemId: string,
): Promise<void> {
    if (!isTauri()) return;
    const dir = PAYLOAD_DIR_BY_TYPE[itemType];
    if (!dir) return;
    try {
        const { exists, remove } = await fs();
        const file = joinPath(path, dir, `${itemId}.json`);
        if (await exists(file)) await remove(file);
    } catch (err) {
        console.warn('deleteItemPayload failed', err);
    }
}

/**
 * Convenience: write the full workspace state in v2 layout. Used by
 * migration and as a fallback.
 */
export async function saveAll(path: string, state: WorkspaceState): Promise<void> {
    if (!isTauri()) return;
    await ensureDir(path);
    await saveMetadata(path, {
        engine: state.engine,
        jobs: state.jobs,
        activeJobId: state.activeJobId,
    });
    if (Array.isArray(state.repo)) {
        await saveRepository(path, state.repo as Array<Record<string, unknown>>);
        for (const item of state.repo as Array<Record<string, unknown>>) {
            const itype = typeof item.type === 'string' ? item.type : '';
            if (itype === 'pipeline' || itype === 'folder' || itype === 'project') continue;
            const payload = (item as { payload?: unknown }).payload;
            if (payload !== undefined) {
                await saveItemPayload(path, itype, item.id as string, payload);
            }
        }
    }
    if (state.pipelineData) {
        for (const [id, pipeline] of Object.entries(state.pipelineData)) {
            await savePipelineFile(path, id, pipeline);
        }
    }
}

// Kept for backwards compatibility - callers that just want to write
// everything in one shot can still call saveWorkspace().
export const saveWorkspace = saveAll;

// Expose for cleanup utilities.
export async function listPipelineFiles(path: string): Promise<string[]> {
    return readDirEntries(joinPath(path, PIPELINES_DIR));
}
