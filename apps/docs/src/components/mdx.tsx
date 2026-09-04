import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';
import { CommandBuilder } from './CommandBuilder';
import { TuiPreview } from './TuiPreview';

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    CommandBuilder,
    TuiPreview,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}

