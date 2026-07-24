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
export { SearchField } from './components/common/SearchField';
export type { SearchFieldProps } from './components/common/SearchField';
export { SearchableSelect } from './components/common/SearchableSelect';
export type {
  SearchableSelectProps,
  SearchableSelectOption,
} from './components/common/SearchableSelect';
export { ChipButton } from './components/common/ChipButton';
export type { ChipButtonProps, ChipButtonVariant } from './components/common/ChipButton';
export {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuAction,
  DropdownMenuSeparator,
} from './components/common/DropdownMenu';
export type {
  DropdownMenuProps,
  DropdownMenuTriggerProps,
  DropdownMenuContentProps,
  DropdownMenuItemProps,
} from './components/common/DropdownMenu';
export { HoverTooltip } from './components/common/HoverTooltip';
export type { HoverTooltipProps, HoverTooltipSide } from './components/common/HoverTooltip';
export {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
} from './components/common/Card';
export { Switch } from './components/common/Switch';
export { Toast, ToastContainer } from './components/common/Toast';
export type { ToastProps, ToastType, ToastAction } from './components/common/Toast';
export { ConfirmDialog } from './components/common/ConfirmDialog';
export { useConfirm } from './components/common/use-confirm.hook';
export type { ConfirmDialogState, ConfirmDialogProps } from './components/common/ConfirmDialog';
export { PageHeader } from './components/common/PageHeader';

// Hooks
export { useToast } from './hooks/useToast';
export type { ToastOptions } from './hooks/useToast';
export { useClickOutside } from './hooks/useClickOutside';

// Utilities
export { cn } from './lib/cn';

// Version
export const UI_VERSION = '0.1.0';
