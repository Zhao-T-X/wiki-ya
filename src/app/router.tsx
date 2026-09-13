import { createHashRouter, Navigate } from 'react-router-dom';

import { App } from '@/app/App';
import { RouteErrorBoundary } from '@/app/RouteErrorBoundary';
import { AskPage } from '@/features/ask/AskPage';
import { GraphPage } from '@/features/graph/GraphPage';
import { HomePage } from '@/features/home/HomePage';
import { ClaimDetailPage } from '@/features/knowledge/ClaimDetailPage';
import { DocumentDetailPage } from '@/features/knowledge/DocumentDetailPage';
import { KnowledgePage } from '@/features/knowledge/KnowledgePage';
import { ResearchPage } from '@/features/research/ResearchPage';
import { ReviewPage } from '@/features/review/ReviewPage';
import { TimelinePage } from '@/features/timeline/TimelinePage';
import { MigrationPage } from '@/features/migration/MigrationPage';
import { SearchPage } from '@/features/search/SearchPage';
import { SettingsPage } from '@/features/settings/SettingsPage';

/**
 * 使用 HashRouter：桌面 WebView 与浏览器预览下深链/reload 都不会 404，
 * 也无需后端 history fallback。
 */
export const router = createHashRouter([
  {
    path: '/',
    element: <App />,
    errorElement: <RouteErrorBoundary />,
    children: [
      { index: true, element: <Navigate to="/home" replace /> },
      { path: 'home', element: <HomePage /> },
      // 兼容旧链接：/inbox 永久跳转到 Home。
      { path: 'inbox', element: <Navigate to="/home" replace /> },
      { path: 'knowledge', element: <KnowledgePage /> },
      { path: 'knowledge/:entityId', element: <KnowledgePage /> },
      { path: 'claims/:claimId', element: <ClaimDetailPage /> },
      { path: 'documents/:documentId', element: <DocumentDetailPage /> },
      { path: 'search', element: <SearchPage /> },
      { path: 'review', element: <ReviewPage /> },
      { path: 'ask', element: <AskPage /> },
      { path: 'research', element: <ResearchPage /> },
      { path: 'graph', element: <GraphPage /> },
      { path: 'timeline', element: <TimelinePage /> },
      { path: 'migration', element: <MigrationPage /> },
      { path: 'settings', element: <SettingsPage /> },
      { path: '*', element: <Navigate to="/home" replace /> },
    ],
  },
]);
