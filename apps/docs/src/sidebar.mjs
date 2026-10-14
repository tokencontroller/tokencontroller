export const sidebar = [
  {
    label: 'Start',
    items: [
      { label: 'Overview', link: '/' },
      'concepts',
      'plans',
      'scope',
      'security',
    ],
  },
  {
    label: 'Set up',
    items: [
      'account/organizations',
      'account/tags',
      'account/projects',
      'account/roles',
      'account/bills',
      'account/cadence',
      'account/token-budgets',
    ],
  },
  {
    label: 'Connect agents',
    items: [
      'collect',
      'collect/claude-code',
      'collect/cli',
      'collect/telemetry',
      'collect/github-action',
      'collect/token-entries',
      'collect/manual',
    ],
  },
  {
    label: 'Place cost',
    items: [
      'attribute/rules',
      'attribute/review',
      'attribute/classifier',
    ],
  },
  {
    label: 'Close and report',
    items: [
      'account/close',
      'report/reports',
      'report/exports',
      'report/checking-a-number',
    ],
  },
  {
    label: 'Self-hosting',
    items: [
      { slug: 'self-hosting', label: 'Run it yourself' },
      'self-hosting/configuration',
      'self-hosting/enterprise',
    ],
  },
  {
    label: 'Reference',
    items: [
      'reference/cli',
      'reference/api',
      'reference/token-entry',
      'reference/bill',
      'reference/data-model',
      'reference/pricing-tables',
      { label: 'Glossary', link: '/glossary' },
      'reference/open-source',
      'changelog',
    ],
  },
];
