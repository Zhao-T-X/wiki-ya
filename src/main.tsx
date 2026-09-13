import React from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';

import { router } from '@/app/router';
import '@/styles.css';

const container = document.getElementById('root');

if (!container) {
  throw new Error('未找到 #root 挂载点，index.html 可能被改动。');
}

createRoot(container).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
);
