# Getting Started Guide

This guide walks you through the visual layout of the Duckle application and helps you build, configure, and execute your first data pipeline using the drag-and-drop editor.

---

## 1. Visual Studio Layout

The Duckle user interface is divided into five main areas:

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │  Toolbar (Run, Stop, Git, Schedule, Context Dropdown, Language)        │
 ├──────────────┬──────────────────────────────────────────┬──────────────┤
 │ Left Sidebar │                                          │ Right Panel  │
 │ Toggle       │                                          │              │
 │              │             Visual Canvas                │  Properties  │
 │ ├──────────┤ │             (Drag-and-Drop)              │   Form       │
 │ │ Palette  │ │                                          │              │
 │ │ / Tree   │ │                                          │   (Config)   │
 │ └──────────┘ │                                          │              │
 ├──────────────┴──────────────────────────────────────────┴──────────────┤
 │  Bottom Panel (Preview data table, SQL Plans, Output Run logs)         │
 └────────────────────────────────────────────────────────────────────────┘
```

* **Toolbar**: Top panel containing action buttons (**Run**, **Stop**, **Save**), and modal controls for scheduling, Git integrations, active Environment contexts, and language translation.
* **Left Sidebar**: Toggles between the **Project Tree** (for managing workspace directories and files) and the **Component Palette** (your catalog of visual nodes).
* **Visual Canvas**: The interactive board where you design pipelines by dragging nodes and drawing connecting lines.
* **Right Panel (Properties)**: Shows configuration options, schema mappings, and validation fields for the currently selected node.
* **Bottom Panel**: Interactive tabs showing data previews, SQL scripts, and run performance charts.

---

## 2. Tutorial: Building a CSV Cleanup Pipeline

We will design a visual pipeline that loads a CSV orders log, filters out pending records, and writes the output directly to a Parquet file.

### Step 1: Placing the Source Node
1. Toggle the **Palette** tab on the Left Sidebar.
2. Expand the **Sources** group, then drag a **CSV** icon onto the **Canvas**.
3. Select the CSV node to open its configuration fields in the **Properties Panel**.
4. In the properties field, click the **Browse** folder icon and select `samples/orders.csv`.
5. Click the green **Autodetect schema** button. 
   * The CSV node will scan the first few rows.
   * Open the **Preview** tab in the **Bottom Panel** to verify that your data columns are displaying correctly.

### Step 2: Placing the Filter Node
1. In the Left Sidebar Palette, expand the **Transforms** section.
2. Drag a **Filter** node onto the canvas, placing it to the right of the CSV node.
3. Click and hold the circle (output port) on the right of the CSV node, drag a line to the circle (input port) on the left of the Filter node, and release the mouse.
4. Select the Filter node. In the **Predicate** property text area, type:
   ```sql
   status = 'paid'
   ```

### Step 3: Placing the Sink Node
1. In the Left Sidebar Palette, expand the **Sinks** section.
2. Drag a **Parquet** node onto the canvas, placing it to the right of the Filter node.
3. Draw a connection line from the Filter's `pass` output port to the Parquet node's input port.
4. Select the Parquet node, type `paid_orders.parquet` in the **Path** property field, and set the **Write Mode** dropdown option to `Overwrite`.

### Step 4: Run the Pipeline
1. Click the green **Run** button in the top toolbar.
2. You will see the nodes light up green one-by-one as data executes. The number of rows processed will stream directly under the connection lines.
3. To stop a pipeline execution mid-run, click the red **Stop** button in the toolbar.
4. Click the Parquet node and look at the **Preview** tab in the **Bottom Panel** to see the resulting data.

---

## 3. Working with Duckie (AI Assistant Sidebar)

You can build pipelines using natural language instead of drawing nodes manually:

1. Click the **Sparkles** icon in the top toolbar. The **Duckie AI Assistant** panel will open on the right side of the screen.
2. In the chat input box, type:
   > "read orders.csv, filter where status is paid, and write to paid.parquet"
3. Click the Send button. Duckie will generate and stream the pipeline structure.
4. Click the **Insert into canvas** button at the bottom of Duckie's response. The canvas will immediately populate with the connected nodes.

---

## 4. Context Environments

You can change environment properties globally using the **Context Dropdown** in the top toolbar:

1. Click the context selector in the toolbar (it defaults to `default`).
2. Switch to another environment context (e.g. `prod` or `dev`).
3. Any node properties referencing a context variable (such as entering `${DATA_DIR}/orders.csv` in a path field) will immediately update their runtime evaluation to point to the new folder location.
