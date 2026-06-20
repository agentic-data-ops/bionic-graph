import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from '../locales/en.json';
import NavBar from '../components/NavBar';
import SearchBar from '../components/SearchBar';
import PropertyPanel from '../components/PropertyPanel';
import * as api from '../api';

i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
});

// ─── NavBar ────────────────────────────────────────

describe('NavBar', () => {
  beforeEach(() => {
    global.fetch = vi.fn(() =>
      Promise.resolve({ ok: true, json: () => Promise.resolve({ graphs: ['default', 'test'] }) })
    );
  });

  it('renders Compact and Extract buttons', async () => {
    render(<NavBar graph="default" setGraph={() => {}} />);
    await waitFor(() => {
      expect(screen.getByText('Compact')).toBeInTheDocument();
      expect(screen.getByText('Extract')).toBeInTheDocument();
    });
  });

  it('opens Add Graph modal on + click', async () => {
    render(<NavBar graph="default" setGraph={() => {}} />);
    fireEvent.click(screen.getByText('+'));
    await waitFor(() => expect(screen.getByText('Add Graph')).toBeInTheDocument());
    expect(screen.getByPlaceholderText('Graph name')).toBeInTheDocument();
  });

  it('opens Compact modal on Compact click', () => {
    render(<NavBar graph="default" setGraph={() => {}} />);
    fireEvent.click(screen.getByText('Compact'));
    expect(screen.getByText(/Compact History/)).toBeInTheDocument();
  });

  it('opens Extract modal on Extract click', () => {
    render(<NavBar graph="default" setGraph={() => {}} />);
    fireEvent.click(screen.getByText('Extract'));
    expect(screen.getByText(/Extract from Markdown/)).toBeInTheDocument();
  });

  it('calls setGraph on selector change', async () => {
    const setGraph = vi.fn();
    render(<NavBar graph="default" setGraph={setGraph} />);
    await waitFor(() => {
      fireEvent.change(screen.getByRole('combobox'), { target: { value: 'test' } });
      expect(setGraph).toHaveBeenCalledWith('test');
    });
  });

  it('toggles theme on click', () => {
    render(<NavBar graph="default" setGraph={() => {}} />);
    fireEvent.click(screen.getByText('☀️'));
    expect(screen.getByText('🌙')).toBeInTheDocument();
  });
});

// ─── SearchBar ──────────────────────────────────────

describe('SearchBar', () => {
  it('defaults to keyword mode', () => {
    render(<SearchBar onSearch={() => {}} />);
    expect(screen.getByText('Keyword').className).toContain('bg-blue-600');
  });

  it('toggles to semantic mode', () => {
    render(<SearchBar onSearch={() => {}} />);
    fireEvent.click(screen.getByText('Semantic'));
    expect(screen.getByText('Semantic').className).toContain('bg-blue-600');
  });

  it('shows advanced panel on toggle', () => {
    render(<SearchBar onSearch={() => {}} />);
    fireEvent.click(screen.getByText(/Advanced/));
    expect(screen.getByText(/Vertex label/)).toBeInTheDocument();
  });

  it('calls onSearch on Enter', () => {
    const onSearch = vi.fn();
    render(<SearchBar onSearch={onSearch} />);
    fireEvent.change(screen.getByPlaceholderText(/search/i), { target: { value: 'test query' } });
    fireEvent.keyDown(screen.getByPlaceholderText(/search/i), { key: 'Enter' });
    expect(onSearch).toHaveBeenCalledWith(expect.objectContaining({ query: 'test query', mode: 'keyword' }));
  });

  it('calls onSearch on search button click', () => {
    const onSearch = vi.fn();
    render(<SearchBar onSearch={onSearch} />);
    fireEvent.change(screen.getByPlaceholderText(/search/i), { target: { value: 'AI' } });
    fireEvent.click(screen.getByText('Search'));
    expect(onSearch).toHaveBeenCalled();
  });
});

// ─── PropertyPanel ──────────────────────────────────

describe('PropertyPanel', () => {
  const vertex = { id: 42, labels: ['person'], type: 'vertex', properties: { name: 'Alice', age: 30 } };
  const edge = { id: 5, label: 'knows', type: 'edge', source: 1, target: 2, properties: { since: '2020' } };

  it('renders vertex ID and labels', () => {
    render(<PropertyPanel item={vertex} type="vertex" onClose={() => {}} />);
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.getByText('person')).toBeInTheDocument();
  });

  it('renders vertex properties', () => {
    render(<PropertyPanel item={vertex} type="vertex" onClose={() => {}} />);
    expect(screen.getByText('Alice')).toBeInTheDocument();
    expect(screen.getByText('30')).toBeInTheDocument();
  });

  it('renders edge source/target', () => {
    render(<PropertyPanel item={edge} type="edge" onClose={() => {}} />);
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
  });

  it('renders edge label in properties', () => {
    const e = { id: 5, label: 'knows', type: 'edge', source: 1, target: 2, properties: { label: 'knows' } };
    render(<PropertyPanel item={e} type="edge" onClose={() => {}} />);
    expect(screen.getByText('knows')).toBeInTheDocument();
  });

  it('calls onClose on ✕ click', () => {
    const fn = vi.fn();
    render(<PropertyPanel item={vertex} type="vertex" onClose={fn} />);
    fireEvent.click(screen.getByText('✕'));
    expect(fn).toHaveBeenCalledOnce();
  });

  it('shows — for empty properties', () => {
    render(<PropertyPanel item={{ id: 99, labels: [], type: 'vertex', properties: {} }} type="vertex" onClose={() => {}} />);
    expect(screen.getByText('—')).toBeInTheDocument();
  });
});

// ─── API module ─────────────────────────────────────

describe('API client', () => {
  beforeEach(() => { global.fetch = vi.fn(); });

  it('health calls GET /health', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({ status: 'ok' }) });
    expect((await api.health()).status).toBe('ok');
  });

  it('keywordSearch sends proper step', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.keywordSearch(['AI', 'engineer'], 'g');
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.steps[0].step).toBe('keywordSearch');
    expect(body.steps[0].keywords).toEqual(['AI', 'engineer']);
  });

  it('semanticSearch sends proper step', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.semanticSearch('find people', 'g');
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.steps[0].step).toBe('semanticSearch');
    expect(body.steps[0].query).toBe('find people');
  });

  it('listGraphs calls GET', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({ graphs: ['a', 'b'] }) });
    expect((await api.listGraphs()).graphs).toEqual(['a', 'b']);
  });

  it('createGraph sends POST with body', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.createGraph('g', true);
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.name).toBe('g');
    expect(body.time_travel).toBe(true);
  });

  it('deleteGraph sends DELETE', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.deleteGraph('g');
    expect(fetch.mock.calls[0][0]).toBe('/graphs/g');
    expect(fetch.mock.calls[0][1].method).toBe('DELETE');
  });

  it('extractDoc sends raw markdown', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.extractDoc('# Hello', 'g');
    const call = fetch.mock.calls[0];
    expect(call[0]).toBe('/extract');
    expect(call[1].body).toBe('# Hello');
    expect(call[1].headers['Content-Type']).toBe('text/markdown');
  });

  it('traverse sends V + out steps', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.traverse(42, 'knows', 'g');
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.steps[0].step).toBe('V');
    expect(body.steps[0].ids).toEqual([42]);
    expect(body.steps[1].step).toBe('out');
  });

  it('throws on non-ok response', async () => {
    fetch.mockResolvedValueOnce({ ok: false, status: 400, text: () => Promise.resolve('err') });
    await expect(api.health()).rejects.toThrow('err');
  });
});
