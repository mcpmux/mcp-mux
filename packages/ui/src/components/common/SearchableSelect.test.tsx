/**
 * Tests for SearchableSelect component
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { SearchableSelect } from './SearchableSelect';

const OPTIONS = [
  { value: 'apple', label: 'Apple', icon: '🍎' },
  { value: 'banana', label: 'Banana', icon: '🍌' },
  { value: 'cherry', label: 'Cherry', icon: '🍒' },
] as const;

describe('SearchableSelect', () => {
  it('renders placeholder when no value selected', () => {
    const mockOnChange = vi.fn();
    render(
      <SearchableSelect
        value=""
        onChange={mockOnChange}
        options={[...OPTIONS]}
        placeholder="Select a fruit"
        testId="fruit-select"
      />
    );

    expect(screen.getByTestId('fruit-select')).toHaveTextContent('Select a fruit');
  });

  it('filters options by typed text', () => {
    const mockOnChange = vi.fn();
    render(
      <SearchableSelect
        value=""
        onChange={mockOnChange}
        options={[...OPTIONS]}
        placeholder="Select a fruit"
        testId="fruit-select"
      />
    );

    fireEvent.click(screen.getByTestId('fruit-select'));

    const searchInput = screen.getByTestId('fruit-select-search');
    fireEvent.change(searchInput, { target: { value: 'ban' } });

    expect(screen.getByTestId('fruit-select-option-banana')).toBeInTheDocument();
    expect(screen.queryByTestId('fruit-select-option-apple')).not.toBeInTheDocument();
    expect(screen.queryByTestId('fruit-select-option-cherry')).not.toBeInTheDocument();
  });

  it('calls onChange when an option is clicked', () => {
    const mockOnChange = vi.fn();
    render(
      <SearchableSelect
        value=""
        onChange={mockOnChange}
        options={[...OPTIONS]}
        placeholder="Select a fruit"
        testId="fruit-select"
      />
    );

    fireEvent.click(screen.getByTestId('fruit-select'));
    fireEvent.click(screen.getByTestId('fruit-select-option-cherry'));

    expect(mockOnChange).toHaveBeenCalledWith('cherry');
    expect(mockOnChange).toHaveBeenCalledTimes(1);
  });

  it('calls onCreateNew when the create-new row is clicked', () => {
    const mockOnChange = vi.fn();
    const mockOnCreateNew = vi.fn();
    render(
      <SearchableSelect
        value=""
        onChange={mockOnChange}
        options={[...OPTIONS]}
        placeholder="Select a fruit"
        onCreateNew={mockOnCreateNew}
        testId="fruit-select"
      />
    );

    fireEvent.click(screen.getByTestId('fruit-select'));
    fireEvent.click(screen.getByTestId('fruit-select-create-new'));

    expect(mockOnCreateNew).toHaveBeenCalledTimes(1);
    expect(mockOnChange).not.toHaveBeenCalled();
  });
});
