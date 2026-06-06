# Duckle User Guide

Welcome to the **Duckle User Guide**! Duckle is an open-source, local-first desktop ETL / ELT studio. It features an intuitive drag-and-drop canvas, a comprehensive properties panel, real-time data previews, and a built-in AI assistant (**Duckie**) that runs entirely on your local machine.

Using Duckle, you can construct visual pipelines to extract, clean, transform, validate, and load data without writing complex SQL scripts or code. Every visual node you place is translated into highly optimized queries behind the scenes, giving you full visibility and speed.

---

## Documentation Navigation

This guide is organized into the following sections to help you get the most out of the Duckle application:

### 1. [Installation & Setup](installation.md)
* How to download and run the Duckle application on **Windows, macOS, and Linux**.
* Running the **Guided Startup Setup** to download database and local AI engines.
* Understanding your **Workspace Folder** structure on disk.

### 2. [Getting Started Guide](getting-started.md)
* Learn how to navigate the **Canvas Interface** and use the **Component Palette**.
* Build your first data pipeline: connecting a CSV file to a Parquet output.
* Open the **Duckie AI Sidebar** to build and update pipelines in plain English.
* Run your pipeline and inspect results in the **Bottom Panel (Previews & SQL Plans)**.

### 3. [Connectors: Sources & Sinks](connectors.md)
* Dragging and dropping source and sink nodes from the Palette.
* Using the **Properties Panel** to map paths, databases, and variables.
* Deep dive into setting up the **CSV/TSV node**, automatic schema scanning, and custom quote/delimiter inputs.
* Summary of available visual connectors for files, databases, object stores, cloud warehouses, and vector databases.

### 4. [Transforms & Data Quality](transforms.md)
* Overview of visual transformation blocks (manipulating columns, filtering rows, aggregating data, and performing lookups).
* Joining tables visually using the interactive **Map Node Editor**.
* Taming messy data with **QA Validators** and routing invalid records to a dedicated **Reject Port**.
* Writing custom scripts directly within **JavaScript, WebAssembly, and SQL UDF nodes** in the properties panel.

### 5. [Execution Controls](engines.md)
* Running pipelines using the **Run** and **Stop** controls.
* Switching between execution backends (DuckDB and SlothDB) in the header.
* How the application pre-installs database extensions so you can work completely offline.

### 6. [Scheduler & Automation](scheduler.md)
* Opening the **Schedule Editor Modal** to trigger pipelines automatically.
* Creating schedules based on **Cron expressions**, **time intervals**, or **File-Watch folders**.
* Tracking execution history, duration, and error reports within the scheduler list.
* Executing saved pipelines headlessly via standard terminal command lines.

### 7. [Desktop Shell & Workspace Git Flow](architecture.md)
* Working with multiple workspace folders.
* Using the built-in **Git Panel** to stage, commit, branch, and push your visual pipeline files.
* Securely managing encrypted connection passwords.
* Interacting with the local AI assistant process panel.

---

## Core Visual Concepts

When using Duckle, you will primarily work with three visual structures:
* **The Canvas**: A large interactive board where you design pipelines by drawing connector lines between handles on nodes.
* **Nodes (Components)**: Visual blocks representing a source (e.g. CSV), a transformation (e.g. Filter), a QA validator, or a target destination (e.g. database table).
* **Ports & Edges**: Connective pins on nodes. Circles on the left are inputs; circles on the right are outputs. Connector lines (edges) carry the data flow.
