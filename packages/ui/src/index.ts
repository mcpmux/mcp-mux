/**
 * McpMux Shared UI Components
 *
 * This package contains reusable React components for the McpMux desktop application.
 */

// Layout components
export { AppShell } from './components/layout/AppShell';
export { Sidebar, SidebarItem, SidebarSection } from './components/layout/Sidebar';
export { StatusBar, StatusBarItem } from './components/layout/StatusBar';

// Common components
export { Button } from './components/common/Button';
export { Input } from './components/common/Input';
export { Card, CardHeader, CardTitle, CardDescription, CardContent } from './components/common/Card';
export { Switch } from './components/common/Switch';

// Utilities
export { cn } from './lib/cn';

// Version
export const UI_VERSION = '0.1.0';
