import type { Edge, Node } from '@xyflow/react';
import type { DuckleNodeData } from './pipeline-types';
import { getManifest } from './workflow-ui/fields/component-manifests';

export type ValidationIssue = {
    id: string;
    severity: 'error' | 'warning';
    code: string;
    message: string;
    nodeId?: string;
    edgeId?: string;
};

export type ValidationResult = {
    issues: ValidationIssue[];
    errorCount: number;
    warningCount: number;
    errorByNode: Record<string, ValidationIssue[]>;
};

const EMPTY: ValidationResult = {
    issues: [],
    errorCount: 0,
    warningCount: 0,
    errorByNode: {},
};

export function validatePipeline(
    nodes: Node<DuckleNodeData>[],
    edges: Edge[],
): ValidationResult {
    if (nodes.length === 0) return EMPTY;

    const issues: ValidationIssue[] = [];
    const push = (i: Omit<ValidationIssue, 'id'>) => {
        issues.push({ id: 'i_' + issues.length, ...i });
    };

    const nodeIds = new Set(nodes.map(n => n.id));

    // ---- Per-node checks ----
    for (const node of nodes) {
        if (node.data.disabled) continue;
        const manifest = getManifest(node.data.componentId);
        if (!manifest) {
            push({
                severity: 'warning',
                code: 'unknown-component',
                message: `Unknown component '${node.data.componentId ?? '?'}'.`,
                nodeId: node.id,
            });
            continue;
        }

        // Required fields populated
        const props = node.data.properties ?? {};
        for (const section of manifest.sections) {
            for (const field of section.fields) {
                if (!field.required) continue;
                const v = props[field.key];
                const empty =
                    v === undefined ||
                    v === null ||
                    v === '' ||
                    (Array.isArray(v) && v.length === 0);
                if (empty) {
                    push({
                        severity: 'error',
                        code: 'missing-required-field',
                        message: `${node.data.label}: '${field.label}' is required.`,
                        nodeId: node.id,
                    });
                }
            }
        }

        // Required inputs connected. Inputs without `optional: true`
        // must have at least one upstream edge of any matching type
        // (we accept the edge regardless of connectionType for now -
        // the picker already enforces compatibility on creation).
        const inputs = manifest.ports?.inputs ?? [];
        const required = inputs.filter(p => !p.optional);
        if (required.length > 0) {
            const hasMain = edges.some(e => e.target === node.id);
            if (!hasMain) {
                push({
                    severity: 'error',
                    code: 'missing-required-input',
                    message: `${node.data.label} has no upstream connection.`,
                    nodeId: node.id,
                });
            }
        }

        // Filter sanity - predicate non-empty if it's a filter
        if (node.data.componentId === 'xf.filter') {
            const pred =
                typeof props.predicate === 'string' ? props.predicate.trim() : '';
            if (!pred) {
                push({
                    severity: 'warning',
                    code: 'empty-filter-predicate',
                    message: `${node.data.label}: predicate is empty - every row will pass.`,
                    nodeId: node.id,
                });
            }
        }

        // Sinks need a path
        if (
            typeof node.data.componentId === 'string' &&
            node.data.componentId.startsWith('snk.')
        ) {
            const path =
                typeof props.path === 'string' ? props.path.trim() : '';
            if (!path) {
                push({
                    severity: 'error',
                    code: 'sink-without-path',
                    message: `${node.data.label}: output path is required.`,
                    nodeId: node.id,
                });
            }
        }
    }

    // ---- Edge checks ----
    for (const e of edges) {
        if (!nodeIds.has(e.source) || !nodeIds.has(e.target)) {
            push({
                severity: 'warning',
                code: 'dangling-edge',
                message: `Edge ${e.id} references a missing node.`,
                edgeId: e.id,
            });
        }
    }

    // ---- Cycle detection on data-flow edges ----
    if (hasCycle(nodes, edges)) {
        push({
            severity: 'error',
            code: 'cycle',
            message: 'Pipeline contains a cycle in the data-flow graph.',
        });
    }

    // ---- Bucket by node id for inline UI ----
    const errorByNode: Record<string, ValidationIssue[]> = {};
    let errorCount = 0;
    let warningCount = 0;
    for (const i of issues) {
        if (i.severity === 'error') errorCount += 1;
        else warningCount += 1;
        if (i.nodeId) {
            (errorByNode[i.nodeId] ??= []).push(i);
        }
    }

    return { issues, errorCount, warningCount, errorByNode };
}

function hasCycle(
    nodes: Node<DuckleNodeData>[],
    edges: Edge[],
): boolean {
    const adj = new Map<string, string[]>();
    const inDegree = new Map<string, number>();
    for (const n of nodes) {
        adj.set(n.id, []);
        inDegree.set(n.id, 0);
    }
    const dataEdges = edges.filter(e => {
        const t = (e.data as { connectionType?: string } | undefined)?.connectionType;
        return !t || t === 'main' || t === 'lookup' || t === 'reject' || t === 'filter';
    });
    for (const e of dataEdges) {
        if (!adj.has(e.source) || !adj.has(e.target)) continue;
        adj.get(e.source)!.push(e.target);
        inDegree.set(e.target, (inDegree.get(e.target) ?? 0) + 1);
    }
    const queue: string[] = [];
    for (const [id, d] of inDegree) if (d === 0) queue.push(id);
    let processed = 0;
    while (queue.length > 0) {
        const id = queue.shift()!;
        processed += 1;
        for (const child of adj.get(id) ?? []) {
            const d = (inDegree.get(child) ?? 0) - 1;
            inDegree.set(child, d);
            if (d === 0) queue.push(child);
        }
    }
    return processed !== nodes.length;
}
