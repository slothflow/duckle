import { createContext } from 'react';
import type { Column } from '../../pipeline-types';

export type FieldContextValue = {
    upstreamSchema: Column[];
    nodeSchema: Column[];
};

export const FieldContext = createContext<FieldContextValue>({
    upstreamSchema: [],
    nodeSchema: [],
});
