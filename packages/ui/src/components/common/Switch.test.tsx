/**
 * Tests for Switch component
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Switch } from './Switch';

describe('Switch', () => {
  it('renders with unchecked state', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute('aria-checked', 'false');
  });

  it('renders with checked state', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={true} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    expect(button).toHaveAttribute('aria-checked', 'true');
  });

  it('calls onCheckedChange when clicked', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    fireEvent.click(button);
    
    expect(mockHandler).toHaveBeenCalledWith(true);
    expect(mockHandler).toHaveBeenCalledTimes(1);
  });

  it('toggles from checked to unchecked', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={true} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    fireEvent.click(button);
    
    expect(mockHandler).toHaveBeenCalledWith(false);
  });

  it('does not call handler when disabled', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} disabled={true} />);
    
    const button = screen.getByRole('switch');
    fireEvent.click(button);
    
    expect(mockHandler).not.toHaveBeenCalled();
  });

  it('applies disabled attribute when disabled', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} disabled={true} />);
    
    const button = screen.getByRole('switch');
    expect(button).toBeDisabled();
  });

  it('applies custom className', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} className="custom-class" />);
    
    const button = screen.getByRole('switch');
    expect(button).toHaveClass('custom-class');
  });

  it('applies data-testid when provided', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} data-testid="test-switch" />);
    
    const button = screen.getByTestId('test-switch');
    expect(button).toBeInTheDocument();
  });

  it('has correct styles for checked state', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={true} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    expect(button.className).toMatch(/bg-\[rgb\(var\(--primary\)\)\]/);
  });

  it('has correct styles for unchecked state', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    expect(button.className).toMatch(/bg-gray-300/);
  });

  it('has disabled styling when disabled', () => {
    const mockHandler = vi.fn();
    render(<Switch checked={false} onCheckedChange={mockHandler} disabled={true} />);
    
    const button = screen.getByRole('switch');
    expect(button.className).toMatch(/opacity-50/);
  });

  it('can be toggled multiple times', () => {
    const mockHandler = vi.fn();
    const { rerender } = render(<Switch checked={false} onCheckedChange={mockHandler} />);
    
    const button = screen.getByRole('switch');
    
    // First click - should call with true
    fireEvent.click(button);
    expect(mockHandler).toHaveBeenCalledWith(true);
    expect(mockHandler).toHaveBeenCalledTimes(1);
    
    // Simulate parent updating the prop
    rerender(<Switch checked={true} onCheckedChange={mockHandler} />);
    
    // Second click - should call with false
    fireEvent.click(button);
    expect(mockHandler).toHaveBeenCalledWith(false);
    expect(mockHandler).toHaveBeenCalledTimes(2);
    
    // Simulate parent updating the prop again
    rerender(<Switch checked={false} onCheckedChange={mockHandler} />);
    
    // Third click - should call with true again
    fireEvent.click(button);
    expect(mockHandler).toHaveBeenCalledWith(true);
    expect(mockHandler).toHaveBeenCalledTimes(3);
  });
});
