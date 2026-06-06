import type { RequestHandler } from './$types';
import { renderFullMarkdown } from '$lib/sdk-docs-markdown';

/**
 * `GET /docs.md` — referencia completa de SDKs en Markdown, renderizada en
 * servidor (pública, sin auth) para que LLMs y crawlers la lean sin ejecutar
 * el SPA. Generada desde `$lib/sdk-docs`.
 */
export const prerender = false;

export const GET: RequestHandler = ({ url }) => {
  const baseUrl = url.origin;
  const body = renderFullMarkdown(baseUrl);
  return new Response(body, {
    headers: {
      'Content-Type': 'text/markdown; charset=utf-8',
      'Cache-Control': 'public, max-age=3600',
      'Access-Control-Allow-Origin': '*'
    }
  });
};
