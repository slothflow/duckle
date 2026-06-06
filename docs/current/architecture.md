# Desktop Shell & Workspace Git Flow

Duckle is a local-first desktop application that integrates a visual editor, local process managers, and a built-in Git client to simplify pipeline version control.

---

## 1. Local-First Application Design

Duckle runs locally on your workstation:
* **The Desktop Shell**: Visual controls are built in React, running inside a Tauri container that interfaces with local databases, file systems, and background processes.
* **Security & Privacy**: No data is sent to external cloud APIs unless you configure a network connector. Your pipeline configuration files, log reports, and credentials remain isolated on your local hard drive.

---

## 2. Visual Workspace Organization

All your work is stored inside a selected **Workspace Folder** on your local machine.

* **Editing Files**: Pipelines are saved as standard JSON canvas documents under `<workspace>/pipelines/`. You can copy, rename, or share these files through your operating system's file manager just like normal files.
* **Workspace Settings**: Active triggers and scheduling queues are kept in a single `<workspace>/schedules.json` file.
* **Credential Encryption**: Saved connection configurations are written to `<workspace>/connections/`. Sensitive values (such as passwords and database access tokens) are encrypted with a unique key located in `<workspace>/.duckle/keys/`.

> [!WARNING]
> Keep your workspace secure by preventing `.duckle/keys/` from being shared or checked into public repositories.

---

## 3. Local AI Subprocess Model (Duckie)

The **Duckie AI Assistant** is designed to keep your pipeline instructions private.

* **Local Inference**: Duckie starts a local server subprocess on `127.0.0.1`.
* **Sidebar Integration**: The Sparks toolbar icon slides open a dedicated chat panel. It displays a status badge indicating whether the local assistant is **Online** (green) or **Offline** (gray).
* **Canvas Insertion**: When the local assistant outputs a pipeline layout, clicking the **"Insert into canvas"** button renders the nodes and wires them automatically.

---

## 4. Built-in Git Client Panel

Duckle features a visual Git client, allowing you to manage branches, track modifications, and commit changes without using the command-line terminal.

### Opening the Git Client
Click the **Git / Branch icon** in the top toolbar to slide open the **Git Panel** on the right side of the screen.

### Key Features
* **Branch Manager**:
  * View your active branch name.
  * Switch branches by selecting one from the dropdown list.
  * Create new branches by clicking **"New Branch"** and typing a name.
* **Tracking Changes**:
  * Displays a list of files that have been modified, deleted, or newly created in your workspace.
  * Click file names to stage changes.
* **Staging and Committing**:
  * Type a commit message into the text box and click **"Stage All & Commit"** to save your pipeline changes.
* **Push and Pull**:
  * Click **"Pull"** to download changes from your remote repository (e.g., GitHub or GitLab).
  * Click **"Push"** to upload your committed changes.
* **Secure Token Prompts**: If your remote repository requires authentication, Duckle prompts for a Personal Access Token (PAT), saving it encrypted under `.duckle/secrets/` (which is automatically excluded from version control).
* **CI Build Badge**: Once you push modifications, the top toolbar displays a live status badge (green check mark for successful builds, red for failures, and yellow for builds in progress) directly mapping your remote GitHub Actions or GitLab CI workflows.
