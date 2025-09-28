<script module>
    import { defineMeta } from '@storybook/addon-svelte-csf';
    import Badge from './Badge.svelte';
  
    const { Story } = defineMeta({
      title: 'Example/Badge',
      component: Badge,
      tags: ['autodocs'],
      argTypes: {
        variant: {
          control: { type: 'select' },
          options: ['info', 'success', 'warning', 'error']
        }
      }
    });
  </script>
  
  <Story name="Info" args={{ variant: 'info', label: 'Info' }} />
  <Story name="Success" args={{ variant: 'success', label: 'Success' }} />
  <Story name="Warning" args={{ variant: 'warning', label: 'Warning' }} />
  <Story name="Error" args={{ variant: 'error', label: 'Error' }} />