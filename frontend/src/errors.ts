/**
 * Translate raw DuckDB / engine error messages into something a
 * human can act on. We try to pattern-match the common ones; anything
 * we don't recognize falls through unchanged so users still see the
 * original.
 */
export function friendlyError(raw: string | undefined): string {
    if (!raw) return '';
    const s = raw.trim();
    let m: RegExpMatchArray | null;

    // "<stage label>: <DuckDB error>" - strip our stage prefix so the
    // patterns below match cleanly. We'll re-add it at the end if it
    // helped.
    let stagePrefix = '';
    const colonIdx = s.indexOf(':');
    if (colonIdx > 0 && colonIdx < 60) {
        const head = s.slice(0, colonIdx);
        if (!/^(Catalog|Binder|Parser|Conversion|IO|Constraint|Out of Memory) Error$/i.test(head)) {
            stagePrefix = head + ': ';
        }
    }
    const body = stagePrefix ? s.slice(stagePrefix.length) : s;

    const wrap = (msg: string) => stagePrefix + msg;

    if ((m = body.match(/Catalog Error: Table with name (\S+) does not exist/i))) {
        return wrap(
            `Upstream view '${m[1]}' doesn't exist yet. Did the previous stage fail, or is the edge disconnected?`,
        );
    }
    if ((m = body.match(/Catalog Error: Schema with name (\S+) does not exist/i))) {
        return wrap(`Schema '${m[1]}' doesn't exist.`);
    }
    if ((m = body.match(/No files found that match the pattern\s+"?([^"]+?)"?\s*$/i))) {
        return wrap(`No file matches '${m[1].trim()}'. Check the path.`);
    }
    if (/Cannot open file ""/i.test(body)) {
        return wrap('No output path set - open this sink and choose a destination file.');
    }
    if ((m = body.match(/IO Error: Cannot open file "([^"]+)"/i))) {
        return wrap(`Can't open '${m[1]}'. Verify the file exists and the app has permission.`);
    }
    if ((m = body.match(/Referenced column "([^"]+)" not found/i))) {
        return wrap(
            `Column '${m[1]}' isn't available at this stage. Re-run autodetect upstream or fix the reference.`,
        );
    }
    if ((m = body.match(/Table "([^"]+)" does not have a column with name "([^"]+)"/i))) {
        return wrap(`Column '${m[2]}' doesn't exist in '${m[1]}'.`);
    }
    if ((m = body.match(/Conversion Error: (.+)/i))) {
        return wrap(`Type conversion failed - ${m[1]}`);
    }
    if (/^Parser Error:/i.test(body)) {
        return wrap(body.replace(/^Parser Error:\s*/i, 'SQL syntax: '));
    }
    if (/^Binder Error:/i.test(body)) {
        return wrap(body.replace(/^Binder Error:\s*/i, 'SQL binding: '));
    }
    if ((m = body.match(/IO Error: (.+)/i))) {
        const detail = m[1];
        if (/access is denied|permission denied/i.test(detail)) {
            return wrap(`File access denied - ${detail}`);
        }
        return wrap(`I/O - ${detail}`);
    }
    if (/Out of Memory/i.test(body)) {
        return wrap(
            'Out of memory - try LIMITing the upstream or running on a smaller sample.',
        );
    }
    if (/Constraint Error/i.test(body)) {
        return wrap(body.replace(/^Constraint Error:\s*/i, 'Constraint failed: '));
    }
    return s;
}
