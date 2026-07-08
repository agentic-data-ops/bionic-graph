import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from '../locales/en.json';
import PropertyPanel from '../components/PropertyPanel';
import * as api from '../api';

i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
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

  it('calls onClose on close button click', () => {
    const fn = vi.fn();
    render(<PropertyPanel item={vertex} type="vertex" onClose={fn} />);
    fireEvent.click(screen.getByRole('button'));
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

  it('graphSearch sends proper step', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    await api.graphSearch('AI engineer', 'g');
    const body = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body.steps[0].step).toBe('search');
    expect(body.steps[0].text).toBe('AI engineer');
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

  it('traverse sends V+both and V+bothE', async () => {
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({ data: [] }) });
    fetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({ data: [] }) });
    await api.traverse(42, 'knows', 'g');
    // First call: V + both
    const body0 = JSON.parse(fetch.mock.calls[0][1].body);
    expect(body0.steps[0].step).toBe('V');
    expect(body0.steps[0].ids).toEqual([42]);
    expect(body0.steps[1].step).toBe('both');
    // Second call: V + bothE
    const body1 = JSON.parse(fetch.mock.calls[1][1].body);
    expect(body1.steps[0].step).toBe('V');
    expect(body1.steps[0].ids).toEqual([42]);
    expect(body1.steps[1].step).toBe('bothE');
  });

  it('throws on non-ok response', async () => {
    fetch.mockResolvedValueOnce({ ok: false, status: 400, text: () => Promise.resolve('err') });
    await expect(api.health()).rejects.toThrow('err');
  });
});
