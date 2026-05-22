# Talend UI architecture - research notes (reference-only)

> Source: read-only inspection of `C:\Talend` on 2026-05-21. No source code, XML, or template bodies were copied. File paths, class names, extension-point IDs, and component-name examples are facts about the install used here for orientation only.

Inspected install: Talend Open Studio for Data Integration (TOS_DI) 8.0.1, Eclipse RCP (Eclipse 4.x, GEF/Draw2D). Talend plugins under `C:\Talend\plugins\` with `org.talend.*` naming; ~110 present (excluding Hadoop-distribution shells and library wrappers). Most ship as JARs; a few (codegen, local component provider, RCP shell, Maven templates) ship exploded - those are the ones I read.

## 1. UI surface map

One primary perspective: `org.talend.rcp.perspective`, declared via the custom extension point `org.talend.core.talendperspectives` (in `org.talend.core`) and set as startup perspective via `org.talend.core.ui.showPerspectiveAtStartup` (in `org.talend.rcp/plugin.xml`). All panels are standard Eclipse views composed into this perspective.

| Functional surface | Eclipse ID | Owning plugin / class |
|---|---|---|
| Job designer (drag-drop DAG, multi-page editor; tab 1 = canvas, tab 2 = generated code) | editor `org.talend.designer.core.ui.MultiPageTalendEditor` | `org.talend.designer.core` |
| Generated code preview (standalone view) | view `org.talend.designer.core.codeView` | class `org.talend.designer.core.ui.views.CodeView` |
| Component properties panel | view `org.talend.designer.core.ui.views.properties.ComponentSettingsView` | `org.talend.designer.core`. Uses Eclipse tabbed-properties (`org.eclipse.ui.views.properties.tabbed`). |
| Job settings (run-level config) | view `org.talend.designer.core.ui.views.jobsettings.JobSettingsView` | `org.talend.designer.core` |
| Component palette | view `org.eclipse.gef.ui.palette_view` (stock GEF) | Content built at runtime from `org.talend.core.components_provider` registry - no Eclipse extension for palette content |
| Repository / metadata browser | view `org.talend.repository.cnf.view` (Eclipse CNF) | `org.talend.repository.view`, class `RepoViewCommonNavigator` |
| Run / debug view (logs + stats + trace) | view `org.talend.designer.runprocess.ui.views.processview` | `org.talend.designer.runprocess`, class `ProcessView` |
| Problems view | view `org.talend.designer.core.ui.views.ProblemsView` | `org.talend.designer.core` |
| Job hierarchy | view `org.talend.designer.core.ui.hierarchy.JobHierarchyViewPart` | `org.talend.designer.core` |
| JVM runtime visualization | various views | `org.talend.designer.runtime.visualization` (heap/JVM-attach/agent extension points) |

**Engine / execution runtime selector.** TOS 8.0.1 ships only Java-on-JVM. Spark distribution plugins (`org.talend.spark.distribution.spark24x/30x/31x`) and Hadoop distribution plugins are present but contribute only `librariesNeeded` and an `IHadoopDistributionService` - there is no global engine-selector UI. Selection happens at job-creation: the New-Job wizard branches DI vs Big Data Batch vs Streaming, filtering the palette accordingly. The Spark/MapReduce/Flink selector inside the Run view is in the paid Big Data Platform edition (not present here). Hadoop-distribution choice is a property on the metadata connection.

## 2. Palette taxonomy

Categories ("families") are **not** in `plugin.xml`. They live inside each component's `<name>_java.xml` manifest, in a `<FAMILIES>` block whose entries are slash-delimited paths (e.g. `Processing/Fields`, `Cloud/Amazon/S3`). The palette tree is built at startup by parsing every manifest. A component may list multiple `<FAMILY>` entries to appear in several places.

Top-level family counts (from the 588 component folders in `C:\Talend\plugins\org.talend.designer.components.localprovider_*\components\`):

| Top-level family | # components |
|---|---|
| Databases | 255 |
| Cloud | 95 |
| ELT | 58 |
| Internet | 47 |
| File | 47 |
| Technical | 24 |
| Business_Intelligence | 24 |
| Processing | 23 |
| Big Data | 20 |
| Orchestration | 15 |
| Business | 15 |
| XML | 13 |
| Logs_Errors | 11 |
| Misc | 10 |
| Data_Quality | 8 |
| Custom_Code | 8 |
| System | 4 |
| DotNET | 2 |
| Unstructured | 1 |

This 588 is only the local provider. Additional components come from the TaCoKit / generic-component provider (`org.talend.sdk.component.studio.provider.TaCoKitComponentsProvider`) and per-connector plugins under `org.talend.studio.components.tcompv0.*` (Salesforce, Snowflake, Jira, NetSuite, Marketo, Splunk, Azure Storage, Google Drive, JDBC).

**Processing taxonomy (your interest).** Only two sub-families in TOS 8:

- `Processing` (root, 11): `tAggregateRow`, `tAggregateSortedRow`, `tConvertType`, `tExternalSortRow`, `tFilterColumns`, `tFilterRow`, `tJoin`, `tMap`, `tReplace`, `tSampleRow`, `tSortRow`.
- `Processing/Fields` (9): `tDenormalize`, `tDenormalizeSortedRow`, `tExtractDelimitedFields`, `tExtractJSONFields`, `tExtractPositionalFields`, `tExtractRegexFields`, `tNormalize`, `tSplitRow`, `tWriteJSONField`.

Against your expectation (map/filter/aggregate/join/window/routing): map = `tMap`; filter = `tFilterRow`/`tFilterColumns`; aggregate = `tAggregateRow`/`tAggregateSortedRow`; join = `tJoin` (and `tMap` via lookup); **windowing is absent** (it lives in Big Data Streaming components, not bundled here); **routing is in `Orchestration`, not Processing** - `tReplicate`, `tFlowToIterate`, `tIterateToFlow`, `tUnite`, plus connector subtypes (FILTER/REJECT/LOOKUP) declared in the manifests.

Orchestration (15): `tFlowToIterate`, `tForeach`, `tInfiniteLoop`, `tIterateToFlow`, `tLoop`, `tPostjob`, `tPrejob`, `tReplicate`, `tSleep`, `tUnite`, `tWaitForFile`, `tWaitForSocket`, `tWaitForSqlData` etc.

ELT (58): per-engine triples `tELT<Dialect>Input` / `tELT<Dialect>Map` / `tELT<Dialect>Output` for Greenplum, MSSql, MySQL, Netezza, Oracle, PostgreSQL, Sybase, Teradata, Vertica, plus generic `tCombinedSQL*`.

File: `File/Input` (15, incl. `tFileInputDelimited`, `tFileInputJSON`, `tFileInputExcel`, `tFileInputXML`, `tFileInputRegex`, `tFileInputPositional`), `File/Output` (13, parallel writers), `File/Management` (13, incl. `tFileCopy`, `tFileDelete`, `tFileList`, `tFileExist`, `tFileArchive`, `tFileCompare`, `tFileRowCount`).

**Where the taxonomy is defined.** Distributed: the path is a string inside each manifest. Component *providers* are registered via the Eclipse extension point `org.talend.core.components_provider` (in `org.talend.core/plugin.xml`), with two ship-with providers (`LocalComponentsProvider` for XML+JET, `TaCoKitComponentsProvider` for the generic framework) and two for user content (`UserComponentsProvider`, `SharedStudioUserComponentProvider`).

**tMap vs regular Processing components.** `tMap` is a normal manifest that delegates to a separate editor plugin: header attribute `EXTENSION="org.talend.designer.mapper"`, plus a parameter with `FIELD="EXTERNAL"`. The `org.talend.designer.mapper` plugin registers via `org.talend.core.external_component` (class `MapperComponent`) and owns its own EMF model (`http://www.talend.org/mapper`). Same pattern for `tXMLMap` (`org.talend.designer.xmlmap`) and `tELT*Map` variants (`org.talend.designer.dbmap`). Mechanically the node is a regular DAG node; the mapping configuration is a sub-document edited in a dedicated editor.

## 3. Component definition model

Three examples from `org.talend.designer.components.localprovider_*\components\`: **source** `tMysqlInput/`, **transform** `tFilterRow/`, **sink** `tFileOutputDelimited/`. Each is a flat directory with the same file family:

1. `<name>_java.xml` - the manifest. Top-level elements:
   - `<HEADER>` with attributes including `VERSION`, `STATUS` (ALPHA/BETA/STABLE), `STARTABLE`, `SCHEMA_AUTO_PROPAGATE`, `DATA_AUTO_PROPAGATE`, `HAS_CONDITIONAL_OUTPUTS`, `PARTITIONING`, optional `EXTENSION="<plugin id>"` (delegates to an external editor).
   - `<FAMILIES>` with one or more `<FAMILY>` paths.
   - `<DOCUMENTATION>` with a help URL.
   - `<CONNECTORS>` - the **port model**. Each `<CONNECTOR>` has `CTYPE` (FLOW, ITERATE, LOOKUP, FILTER, REJECT, SUBJOB_OK/ERROR, COMPONENT_OK/ERROR, RUN_IF), `MAX_INPUT`, `MAX_OUTPUT`, optional `BASE_SCHEMA`, optional `COMPONENT="<peer>"` to bind a partner, optional `COLOR`/`LINE_STYLE` for the canvas. FLOW with MAX_INPUT=0 = source; MAX_OUTPUT=0 = sink. Reject ports are a second FLOW connector named "REJECT" with dotted style. Lookup ports are CTYPE=LOOKUP.
   - `<PARAMETERS>` - the typed property model. Each `<PARAMETER>` has `NAME`, `FIELD` (widget type - `SCHEMA_TYPE`, `CLOSED_LIST`, `TABLE`, `TEXT`, `FILE`, `PASSWORD`, `EXTERNAL`, `PROPERTY_TYPE`, `MEMO_SQL`, `CHECK`), `REQUIRED`, `NUM_ROW` (form grid row), plus parameter-specific children: `<ITEMS>`/`<ITEM>` for closed lists (with `SHOW_IF`/`NOT_SHOW_IF` expressions that can reference other params, including dot-paths into table cells like `CONDITIONS.INPUT_COLUMN.TYPE`), `<TABLE>`/`<COLUMN>` for static schemas, `<DEFAULT>` for defaults.
   - `<ADVANCED_PARAMETERS>` (optional) - moved to an "Advanced settings" tab.
   - `<CODEGENERATION>` (optional) - declares imports the generated code will need.

2. `<name>_begin.javajet`, `<name>_main.javajet`, `<name>_end.javajet`, optionally `<name>_finally.javajet`, optionally `<name>_<phase>.inc.javajet` includes - **JET templates** emitting Java code. Phases: begin = once before rows; main = per-row body; end = once after rows; finally = clean-up. The JET header declares Java imports and binds `argument` to a `CodeGeneratorArgument`, from which the template pulls `INode` (the configured node) and the upstream `IMetadataTable` (schema). Templates are therefore **schema-aware at generation time** via `node.getMetadataList()`.

3. `<name>_icon32.png` - palette icon (32 px). Some ship a `_white.png` for theme support.

4. `<name>_messages.properties` (+ ~20 locale variants) - i18n labels. Manifest carries technical names; localized display strings come from this bundle.

5. Optional extras: SQL templates (`*.sqltemplate`) for ELT; `*.skeleton` shared JET fragments (e.g. `tMap_commons.skeleton`); license files (e.g. `tMysqlInput/mysql_license`).

**Schema-aware properties.** Yes, at two levels. At design time `SCHEMA_AUTO_PROPAGATE` on the header controls whether the component inherits a schema from FLOW input, and form widgets like `PREV_COLUMN_LIST` read the upstream `IMetadataTable`. Visibility expressions (`SHOW_IF` / `NOT_SHOW_IF`) can reference column types. At codegen time the JET template re-resolves the schema via `node.getMetadataList()` and emits per-column code.

**Linking templates to manifest.** Nothing explicit. The codegen `CodeGenerator` discovers templates by directory co-location and the `<name>_<phase>.javajet` naming convention; no `<TEMPLATE>` element points at template files.

**Adding a new component.** Create a folder under a registered components-provider directory (legacy path: `components/` under a plugin that registers `LocalComponentsProvider`). Drop in (a) `<name>_java.xml`, (b) `<name>_begin/main/end.javajet`, (c) `<name>_icon32.png`, (d) `<name>_messages.properties`. No Studio recompilation: manifests and templates are parsed/expanded at runtime; `Ctrl+Shift+F3` triggers `RefreshTemplatesAction`.

## 4. Plugin / extension model

Everything is an Eclipse OSGi plugin. There are no separate JAR drop-in folders for components; "user components" live in a configurable user folder watched by `UserComponentsProvider`.

Key Talend-defined extension points (mostly in `org.talend.core/plugin.xml`):

| Extension point | Purpose |
|---|---|
| `org.talend.core.components_provider` | Register a component source (folder + provider class). |
| `org.talend.core.external_component` | Component delegates its UI to a custom Java editor (tMap, tXMLMap, tELTMap). |
| `org.talend.core.component_definition` | Programmatic component definitions (the TaCoKit path bypasses XML). |
| `org.talend.core.componentFilter` | Hide components from the palette conditionally. |
| `org.talend.designer.core.generators` | Register controller generators for parameter types. |
| `org.talend.designer.core.process_provider` | Register new kinds of jobs/processes. |
| `org.talend.designer.core.replace_nodes` | Hook to substitute nodes at code-gen time. |
| `org.talend.designer.codegen.additional_jetfile` | Add extra JET templates to the generation pass. |
| `org.talend.core.talendperspectives` | Register additional perspectives. |
| `org.talend.core.repository.repository_node_provider` | Add a node type to the Repository tree. |
| `org.talend.core.routines_provider` | Add custom Java routines (helper code in jobs). |

**Two ways third parties ship a component family.** (a) **Legacy XML+JET**: ship a plugin contributing to `org.talend.core.components_provider`, one directory per component as in §3. Examples in this install: `org.talend.designer.alfrescooutput`, `org.talend.designer.filemultischemas`, `org.talend.designer.fileoutputxml`, `org.talend.designer.scd`, `org.talend.designer.rowgenerator`, `org.talend.designer.webservice` - each a thin plugin primarily shipping component folders. (b) **TaCoKit / Component-Kit**: ship a plugin with an annotated-Java component JAR; the single `TaCoKitComponentsProvider` picks them up via the same extension point. Newer connectors (Snowflake, Salesforce, Jira, NetSuite, Marketo, Splunk, Azure Storage, Google Drive, JDBC) use this path: each ships as `org.talend.studio.components.tcompv0.<name>` (integration shell) plus `org.talend.components.<name>` (component code). Generic glue templates live at `C:\Talend\plugins\org.talend.designer.codegen_*\jet_stub\generic\`.

No Studio recompilation is required for either path.

## 5. Codegen architecture

- **Template engine: Eclipse JET** (Java Emitter Templates, the same engine EMF generators use). Files end `.javajet`; declare Java imports in a `<%@ jet imports="..."%>` header; use `<% ... %>` scriptlets and `<%= ... %>` expressions. The studio expands JET → Java → bytecode at startup; those compiled templates then emit the *generated job code*. Shared fragments use `.skeleton` and `.inc.javajet` (included via JET `<%@ include %>`).
- **Template locations.** Per-component templates sit alongside the manifest. Engine-level "wiring" templates that wrap all per-component output into a runnable class live under `C:\Talend\plugins\org.talend.designer.codegen_*\jet_stub\`: `header.javajet`, `footer.javajet`, `subprocess_header.javajet`, `subprocess_footer.javajet`, `iterate_subprocess_*.javajet`, `subtree_begin.javajet`, `context.javajet`, `default_template.javajet`, `handle_rejects_start.javajet` / `handle_rejects_end.javajet`. A `generic/` subdirectory holds the TaCoKit wrapper templates (`component_begin/main/end/finally.javajet`, `component_process_data_*.javajet`, `component_util_*.javajet`).
- **Naming convention.** `<name>_<phase>.javajet` where phase ∈ {begin, main, end, finally}. `.inc.javajet` for include-only fragments. `.skeleton` for shared snippets.
- **Output languages.** TOS_DI 8.0.1 generates **Java only**. Per-job Maven projects are emitted (templates in `org.talend.designer.maven` / `org.talend.designer.maven.job` plugins via `mavenSetting` / `mavenPom` extension points). Spark/Big-Data editions add additional template sets emitting Java/Scala Spark - those plugins are not in this install. ELT does emit SQL, but as `<%= %>` expressions writing into a Java string-builder that the generated runtime then executes.
- **When codegen runs.** On save/dirty events the editor schedules a background regeneration that populates the Code view tab live. A fuller regeneration runs at Run/Build (F6 to run, F4 to focus the run view). Cached generated code lives in `.JETEmitters` and per-project `.Java/` source folders; Maven then compiles and runs.
- **Generated job size.** From the template structure (header + per-node begin/main/end + subprocess wrappers + footer) plus the Maven scaffolding, a trivial 3-node job (Input → Map → Output) compiles to roughly 400-800 lines of one Java class - per-row loop, reject handlers, log4j/stats hooks, context lookups, try/finally. Scales roughly linearly with node count.

## Duckle implications

- **Adopt: one extension point for component providers.** Talend's `org.talend.core.components_provider` is the single registration surface - everything else (palette, codegen, properties UI) reads from the resulting in-memory registry. Expose exactly one well-typed entry point and let third parties supply N components from a single registration.
- **Adopt: manifest-first, code-template-second component layout.** A directory-per-component with `manifest.{toml,yaml,json}` + per-phase code templates is human-scannable, diff-friendly, self-locating. Talend's file-family convention (icon + manifest + per-phase templates + i18n bundle in one folder) is a good baseline. A strongly typed manifest beats Talend's stringly-typed XML.
- **Adopt: three-level component model.** Connectors (port topology), parameters (typed form fields), and an `EXTENSION` delegation for sub-editors. Separates topology from configuration from "this is too complex for a form, give it a dedicated editor." tMap-class components are too important to omit.
- **Differ: don't use JET, don't generate Java.** JET → Java → bytecode is Talend's deepest legacy debt. Use a single IR (logical plan → DuckDB SQL, Arrow expressions, or Substrait) and let the engine compile it. Templates that emit a textual host language are a tarpit. A "generated source" tab can still exist for the user - derived, not authored.
- **Differ: palette taxonomy should be central, not distributed.** Talend's `<FAMILY>` strings buried in every manifest make global category renames painful. A central `palette.toml` keyed by component ID gives you one place to refactor.
- **Adopt: schema propagation at design time *and* codegen time.** Form fields know upstream column names; the code generator re-resolves the schema when emitting code. Don't push schemas into the manifest at author time - compute them from the upstream graph.
- **Adopt: one perspective, ~7 panels.** Canvas multi-page editor + palette + repository tree + properties + code preview + run/logs + problems is a complete workbench. Talend doesn't over-fragment, and that's worth copying.
- **Open question: engine selector.** Talend punts by branching at job-creation time (DI vs Big Data Batch vs Streaming). For Duckle's DuckDB-first, optionally-Spark/Polars story we probably want a per-job toggle in the canvas header, not buried in a wizard.
- **Open question: 800 components is a feature *and* a curse.** ~250 of Talend's components are dialect variants of the same two ideas (Input/Output). A single parameterized JDBC component covers them all - but costs discoverability ("I want tMySQLInput specifically"). Decide between a virtual palette (one component, dialect-as-property) plus aliases, or many thin per-dialect facades pointing to one implementation.
