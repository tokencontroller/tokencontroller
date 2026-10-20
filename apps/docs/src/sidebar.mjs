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
    label: 'API',
    items: [
      { slug: 'reference/api', label: 'HTTP API' },
      'reference/api/messages',
      'reference/api/bills',
      'reference/api/estimates',
      'reference/api/tags',
      'reference/api/reports',
      'reference/api/organization',
      'reference/api/sign-in',
      'reference/api/mcp',
    ],
  },
  {
    label: 'Set up',
    items: [
      'account/organizations',
      'account/tags',
      'account/attributes',
      'account/roles',
      'account/estimates',
      'account/cutoff',
      'account/submit',
      'account/price-table',
      'account/bills',
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
    label: 'Assign cost',
    items: [
      'attribute/rules',
      'attribute/review',
    ],
  },
  {
    label: 'Reports',
    items: [
      'report/reports',
      'report/deviation',
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
