import type { RequestHandler } from './$types';
import { renderLlmsIndex } from '$lib/sdk-docs-markdown';

/**
 * `GET /llms.txt` — índice conciso (convención llms.txt) que apunta a la
 * referencia completa en `/docs.md`. Público, sin auth.
 */
export const prerender = false;

export const GET: RequestHandler = ({ url }) => {
  const body = renderLlmsIndex(url.origin);
  return new Response(body, {
    headers: {
      'Content-Type': 'text/plain; charset=utf-8',
      'Cache-Control': 'public, max-age=3600',
      'Access-Control-Allow-Origin': '*'
    }
  });
};
