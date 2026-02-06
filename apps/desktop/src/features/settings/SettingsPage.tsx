import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  Button,
} from '@mcpmux/ui';
import {
  Sun,
  Moon,
  Monitor,
  FileText,
  FolderOpen,
  Loader2,
} from 'lucide-react';
import { useAppStore, useTheme } from '@/stores';
import { UpdateChecker } from './UpdateChecker';

export function SettingsPage() {
  const theme = useTheme();
  const setTheme = useAppStore((state) => state.setTheme);
  const [logsPath, setLogsPath] = useState<string>('');
  const [openingLogs, setOpeningLogs] = useState(false);

  // Load logs path on mount
  useEffect(() => {
    const loadLogsPath = async () => {
      try {
        const path = await invoke<string>('get_logs_path');
        setLogsPath(path);
      } catch (error) {
        console.error('Failed to get logs path:', error);
      }
    };
    loadLogsPath();
  }, []);

  const handleOpenLogs = async () => {
    setOpeningLogs(true);
    try {
      await invoke('open_logs_folder');
    } catch (error) {
      console.error('Failed to open logs folder:', error);
    } finally {
      setOpeningLogs(false);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold">Settings</h1>
        <p className="text-[rgb(var(--muted))]">Configure McpMux preferences.</p>
      </div>

      {/* Updates Section */}
      <UpdateChecker />

      {/* Appearance Section */}
      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>Customize the look and feel of McpMux.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">Theme</label>
              <div className="flex gap-2 mt-2" data-testid="theme-buttons">
                <Button
                  variant={theme === 'light' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('light')}
                  data-testid="theme-light-btn"
                >
                  <Sun className="h-4 w-4 mr-2" />
                  Light
                </Button>
                <Button
                  variant={theme === 'dark' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('dark')}
                  data-testid="theme-dark-btn"
                >
                  <Moon className="h-4 w-4 mr-2" />
                  Dark
                </Button>
                <Button
                  variant={theme === 'system' ? 'primary' : 'secondary'}
                  size="sm"
                  onClick={() => setTheme('system')}
                  data-testid="theme-system-btn"
                >
                  <Monitor className="h-4 w-4 mr-2" />
                  System
                </Button>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Logs Section */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <FileText className="h-5 w-5" />
            Logs
          </CardTitle>
          <CardDescription>View application logs for debugging and troubleshooting.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="space-y-4">
            <div>
              <label className="text-sm font-medium">Log Files Location</label>
              <p className="text-sm text-[rgb(var(--muted))] mt-1 font-mono bg-surface-secondary rounded px-2 py-1" data-testid="logs-path">
                {logsPath || 'Loading...'}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                onClick={handleOpenLogs}
                disabled={openingLogs}
                data-testid="open-logs-btn"
              >
                {openingLogs ? (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                ) : (
                  <FolderOpen className="h-4 w-4 mr-2" />
                )}
                Open Logs Folder
              </Button>
            </div>
            <p className="text-xs text-[rgb(var(--muted))]">
              Logs are rotated daily. Each file contains detailed debug information including thread IDs and source locations.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
