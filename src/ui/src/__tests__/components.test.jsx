import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from '../locales/en.json';

i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
});

describe('SearchBar', () => {
  it('renders keyword and semantic buttons', async () => {
    const SearchBar = (await import('../components/SearchBar')).default;
    render(<SearchBar onSearch={() => {}} />);
    expect(screen.getByText('Keyword')).toBeInTheDocument();
    expect(screen.getByText('Semantic')).toBeInTheDocument();
  });

  it('calls onSearch on click', async () => {
    const onSearch = vi.fn();
    const SearchBar = (await import('../components/SearchBar')).default;
    render(<SearchBar onSearch={onSearch} />);
    const input = screen.getByPlaceholderText(/search/i);
    fireEvent.change(input, { target: { value: 'AI engineer' } });
    fireEvent.click(screen.getByText('Search'));
    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({ query: 'AI engineer', mode: 'keyword' })
    );
  });
});

describe('PropertyPanel', () => {
  it('renders vertex properties', async () => {
    const PropertyPanel = (await import('../components/PropertyPanel')).default;
    const item = { id: 42, labels: ['person'], type: 'vertex', properties: { name: 'Alice', age: 30 } };
    render(<PropertyPanel item={item} type="vertex" onClose={() => {}} />);
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('30')).toBeInTheDocument();
  });

  it('renders edge source/target', async () => {
    const PropertyPanel = (await import('../components/PropertyPanel')).default;
    const item = { id: 1, label: 'knows', type: 'edge', source: 10, target: 20, properties: {} };
    render(<PropertyPanel item={item} type="edge" onClose={() => {}} />);
    expect(screen.getByText('10')).toBeInTheDocument();
    expect(screen.getByText('20')).toBeInTheDocument();
  });
});
