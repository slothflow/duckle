# Installation & App Setup

Duckle is a lightweight desktop app that runs entirely on your local machine. Because it does not run in the cloud, there are no remote servers to configure, and all your work remains completely secure.

---

## 1. Running the Desktop Application

Download the application package for your operating system:

* **Windows**: Run the `Duckle-windows-x64.exe` setup helper. 
  * *Unsigned App Warning*: Windows SmartScreen may block launch. Click **"More info"** and then select **"Run anyway"**.
* **macOS**: Extract and open `Duckle-macos-arm64` (for Apple Silicon) or the Intel version.
  * *Gatekeeper Bypass*: Right-click the app icon in Finder and select **"Open"** to bypass security alerts.
* **Linux**: Ensure WebKitGTK (`libwebkit2gtk-4.1-0`) is installed on your package manager, then run `chmod +x Duckle-linux-x64 && ./Duckle-linux-x64`.

---

## 2. Guided Startup Setup Modal

The first time you open Duckle, the **Engine Setup Modal** will appear to help download the necessary engine tools:

![Engine Setup Modal](file:///d:/Repos/GitHub/SouravRoy-ETL/duckle/docs/assets/hero.svg)

1. **DuckDB Database Engine (Required)**
   * **Visual Action**: Click the **Install** button.
   * **Role**: Powers all the SQL compilations, database attachments, schema reads, and file execution processes.
   * **Size**: ~30 MB (plus extension libraries).
   * **Estimated Time**: ~30 seconds.
2. **Duckie AI Assistant (Optional)**
   * **Visual Action**: Click the **Install** button.
   * **Role**: Downloads the **Qwen 2.5 Coder 1.5B** local AI model weights and its runner. This activates the chat window sidebar for offline pipeline generation.
   * **Size**: ~1.1 GB.
   * **Estimated Time**: 5–10 minutes depending on your internet connection.

> [!TIP]
> If you ever need to redownload these engines or reset their installation status, close Duckle, navigate to your platform's configuration folder (Windows: `%APPDATA%\io.duckle.app\engines\`), delete the contents, and launch the application again.

---

## 3. Selecting a Workspace Folder

After setting up the engines, the **Workspace Picker Modal** will ask you to select a folder on your local drive. 

* Click **"Browse / Select Folder"** to open your operating system's native folder dialog.
* Select an empty directory or choose an existing Duckle workspace.

### What is inside the Workspace Folder?
Your workspace is organized into simple, human-readable files that make version control easy:
* **`pipelines/`**: Stores your canvas designs as JSON files. You can save, copy, or rename these in your operating system's file manager.
* **`connections/`**: Holds saved logins and API keys. Sensitive properties are automatically encrypted with a workspace-specific key.
* **`contexts/`**: Contains environment configurations (such as different file paths for Developer and Production modes).
* **`routines/`**: Holds custom SQL files you can reference inside canvas nodes.
* **`documents/`**: A folder for markdown notes that you can read inside the app.
* **`schedules.json`**: Keeps track of active background timers.
* **`run-history/`**: Stores visual execution reports, status metrics, and row count audits for every run.
