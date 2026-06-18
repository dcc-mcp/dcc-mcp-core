import {
  RiDownloadCloudLine,
  RiFileCopyLine,
  RiFolderOpenLine,
  RiRefreshLine,
} from '@remixicon/react';
import { Button } from '../../components/ui/button';
import type {
  OpenApiOperationRow,
  OpenApiSource,
  OpenApiSpec,
  Translator,
} from '../../admin-types';
import {
  OpenApiInspectorPanel,
  PanelHeader,
  StatusLine,
} from '../../admin-ui-core';

export type OpenApiPanelProps = {
  source: OpenApiSource;
  spec: OpenApiSpec | null;
  raw: string;
  operations: OpenApiOperationRow[];
  notice: string;
  updatedAt: string;
  error?: string;
  onCopy: (text: string, label: string) => void;
  onDownload: () => void;
  onShowGatewaySpec: () => void;
  onRefresh: () => void;
  t: Translator;
};

export function OpenApiPanel({
  source,
  spec,
  raw,
  operations,
  notice,
  updatedAt,
  error,
  onCopy,
  onDownload,
  onShowGatewaySpec,
  onRefresh,
  t,
}: OpenApiPanelProps) {
  return (
    <section className="panel active openapi-panel" data-panel="openapi">
      <PanelHeader
        title={t('openapi.title')}
        meta={t('openapi.meta')}
        action={(
          <>
            <Button asChild variant="outline" size="sm">
              <a href={source.docsUrl} target="_blank" rel="noopener noreferrer">
                <RiFolderOpenLine data-icon="inline-start" aria-hidden="true" />
                {t('openapi.action.openReference')}
              </a>
            </Button>
            <Button asChild variant="outline" size="sm">
              <a href={source.specUrl} target="_blank" rel="noopener noreferrer">
                <RiFolderOpenLine data-icon="inline-start" aria-hidden="true" />
                {t('openapi.action.specJson')}
              </a>
            </Button>
            <Button variant="outline" size="sm" type="button" disabled={!raw} onClick={() => onCopy(raw, 'OpenAPI spec JSON')}>
              <RiFileCopyLine data-icon="inline-start" aria-hidden="true" />
              {t('openapi.action.copyJson')}
            </Button>
            <Button variant="outline" size="sm" type="button" disabled={!raw} onClick={onDownload}>
              <RiDownloadCloudLine data-icon="inline-start" aria-hidden="true" />
              {t('openapi.action.downloadJson')}
            </Button>
            {source.kind === 'instance' ? (
              <Button variant="secondary" size="sm" type="button" onClick={onShowGatewaySpec}>
                <RiFolderOpenLine data-icon="inline-start" aria-hidden="true" />
                {t('openapi.action.gatewaySpec')}
              </Button>
            ) : null}
            <Button type="button" size="sm" onClick={onRefresh}>
              <RiRefreshLine data-icon="inline-start" aria-hidden="true" />
              {t('action.refresh')}
            </Button>
          </>
        )}
      />
      <StatusLine text={notice || updatedAt} error={error} />
      <OpenApiInspectorPanel
        spec={spec}
        raw={raw}
        operations={operations}
        source={source}
        labels={{
          emptyDocument: t('openapi.empty.document'),
          openapi: t('openapi.metric.openapi'),
          version: t('openapi.metric.version'),
          paths: t('openapi.metric.paths'),
          operations: t('openapi.metric.operations'),
          schemas: t('openapi.metric.schemas'),
          tags: t('openapi.metric.tags'),
          operationsSection: t('openapi.section.operations'),
          emptyOperations: t('openapi.empty.operations'),
          linksSection: t('openapi.section.links'),
          body: t('openapi.label.body'),
          noBody: t('openapi.label.noBody'),
          params: (count) => t('openapi.label.params', { count }),
          responses: (codes) => t('openapi.label.responses', { codes }),
          noResponses: t('openapi.label.noResponses'),
        }}
        t={t}
      />
    </section>
  );
}
