# Scheduler & Automation

Duckle includes a visual scheduling panel to automatically run pipelines at specific times, set intervals, or in response to local file modifications.

---

## 1. Using the Schedule Editor

To manage automated triggers for your visual pipelines:

1. Click the **Calendar / Clock icon** in the top toolbar to open the **Schedule Editor Modal**.
2. Click the **"Add Schedule"** button to create a new trigger.
3. Configure the trigger parameters:
   * **Name**: Assign a title to describe this schedule.
   * **Pipeline**: Select which visual canvas file inside your workspace to run.
   * **Enabled Switch**: Toggle the switch to active (green) or inactive (gray).
   * **Trigger Type**: Select between Cron, Interval, or File Watch.

---

## 2. Trigger Type Configurations

You can configure three visual trigger types:

### Cron Trigger
* **Setup**: Enter standard cron string schedules (e.g. `0 2 * * *` to run daily at 2:00 AM).
* **Feedback**: The modal displays a text preview indicating when the next execution will occur.

### Interval Trigger
* **Setup**: Select your frequency value (e.g., `15`) and choose a time unit dropdown (Seconds, Minutes, Hours, Days).
* **Cadence**: Duckle schedules the next execution by adding your frequency value to the completion time of the previous run.

### File Watch Trigger
* **Setup**: Enter an absolute path to a folder or file on your disk (e.g., `/Users/username/data/inbox`).
* **Recursive Checkbox**: Check this box if you want Duckle to watch subdirectories.
* **Debounce Buffer**: When changes are detected, Duckle waits **2 seconds** before triggering. This ensures that large files are fully written by other programs before the pipeline begins processing.

---

## 3. Monitoring Scheduled Runs

The Schedule Editor displays a list of saved automation configurations and their status:

* **Last Run**: Timestamps of the last execution.
* **Duration**: Shows how long the pipeline took to execute in milliseconds.
* **Status Badge**: Displays a green **Success** or red **Failed** badge.
* **Error Logs**: If a run fails, hover over or click the failed status to view the error detail.
* **Next Run**: Displays the calculated timestamp of the next planned run (not applicable to File Watch triggers).

---

## 4. Headless Scheduler (CLI mode)

Schedules require the Duckle desktop application to remain open. If you want to deploy pipelines to a headless server or utilize system-level background schedulers (like macOS Launchd, Linux systemd, or Windows Task Scheduler):

1. Save your pipeline visually in Duckle.
2. In your server shell terminal, schedule this command:
   ```bash
   duckle run --workspace "/path/to/workspace" --pipeline "orders_etl"
   ```
This command runs your visual pipelines headlessly without opening the desktop interface, automatically saving row summaries and writing error logs to your workspace folder.
