# Transforms & Data Quality

Transforms let you modify column schemas, clean up values, aggregate counts, or perform joins. You can also run visual validation tests to check data quality.

---

## 1. Using the Map Node Editor

The **Map** node is a visual join and column mapping editor. To use it:

1. Drag a **Map** node onto the canvas and connect your main input stream to it.
2. Drag up to 3 separate lookup sources (such as a SQLite reference table or a CSV) and connect their output handles to the Map node.
3. Select the Map node and click **"Open Mapper Editor"** in the properties panel to load the visual mapper interface:
   * **Join Configurations**: Use the dropdown menus to select the join style (Left Join or Inner Join) and define which columns match between the main stream and lookups.
   * **Field Mappings**: Map output fields by dragging columns from input lists to the output schema.
   * **Expressions**: Write custom SQL-style formulas (such as `upper(first_name) || ' ' || upper(last_name)`) directly in the mapping rows.

---

## 2. Enforcing Data Quality (QA Validators)

Duckle's data validation nodes split data flow based on rules you configure. Validator nodes feature two distinct output handles on the right:

```text
                     ┌───────────────┐
                     │  QA Validator │
  [Input Stream] ───►●               ●───► [Pass Port] (Valid Rows)
                     │               │
                     │               ●───► [Reject Port] (Invalid Rows)
                     └───────────────┘
                                   (Red circle)
```

* **Pass Port (Top Handle)**: Carries rows that successfully pass all validation checks.
* **Reject Port (Bottom Red Handle)**: Collects rows that failed.

### How to Isolate Bad Records
To create a Dead Letter Queue (DLQ):
1. Connect a validator node (such as the **Not-Null Check** or **Range Check**).
2. Wire the top `pass` handle to your primary sink (like a PostgreSQL database).
3. Wire the bottom red `reject` handle to a separate CSV or JSON file named `rejected_records.csv`.
4. When you execute the pipeline, the bad rows will be isolated, allowing you to review errors without stopping the run.

---

## 3. Custom Code Editors (UDFs)

When your logic cannot be built using standard visual blocks, you can paste custom scripts directly inside Duckle:

### JavaScript & WebAssembly Nodes
1. Drag a **JavaScript UDF** or **WASM UDF** node onto the canvas and connect it.
2. Select the node to open the properties panel.
3. Click the **"Code Editor"** tab inside the Properties Panel.
4. Type or paste your code. For JavaScript:
   ```javascript
   function transform(row) {
     row.discounted_price = row.price * 0.9;
     return row;
   }
   ```
5. Click **"Save Script"**. The script runs securely on each row when the pipeline executes.

### Shell Execution Node
1. Drag the **Shell Node** onto the canvas.
2. Enter your script command (e.g., calling a local Python file: `python script.py`).
3. Set the **Timeout (ms)** input field to automatically kill the script if it hangs.
4. The node outputs the script's results, writing `{stdout, stderr, exit_code, duration_ms}` directly into columns for downstream nodes to process.
