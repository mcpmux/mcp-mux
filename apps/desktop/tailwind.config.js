/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{js,ts,jsx,tsx}',
    '../../packages/ui/src/**/*.{js,ts,jsx,tsx}',
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // McpMux brand colors — terracotta/warm palette
        primary: {
          50: '#fef7f4',
          100: '#fde8df',
          200: '#fbd0be',
          300: '#f6ab8a',
          400: '#E8956A',
          500: '#DA7756',
          600: '#C96442',
          700: '#B8553A',
          800: '#8B3D20',
          900: '#6B2E18',
          950: '#3D1A0D',
        },
        // McpMux extended brand palette
        mcpmux: {
          terracotta: '#DA7756',
          sienna: '#B8553A',
          deep: '#C2593A',
          warm: '#E8956A',
          ember: '#C96442',
          amber: '#D4945A',
          cream: '#FDF2E9',
          'cream-deep': '#F5E0D0',
          dark: '#1A120E',
          'dark-surface': '#2D1A12',
          ochre: '#C68B59',
          burnt: '#8B3D20',
        },
        // Surface colors (uses CSS variables)
        surface: {
          DEFAULT: 'rgb(var(--surface))',
          dim: 'rgb(var(--surface-dim))',
          hover: 'rgb(var(--surface-hover))',
          active: 'rgb(var(--surface-active))',
          elevated: 'rgb(var(--surface-elevated))',
          overlay: 'rgb(var(--surface-overlay))',
        },
        border: {
          DEFAULT: 'rgb(var(--border))',
          subtle: 'rgb(var(--border-subtle))',
        },
        muted: {
          DEFAULT: 'rgb(var(--muted))',
          foreground: 'rgb(var(--muted-foreground))',
        },
        background: 'rgb(var(--background))',
        foreground: 'rgb(var(--foreground))',
        card: {
          DEFAULT: 'rgb(var(--card))',
          foreground: 'rgb(var(--card-foreground))',
        },
        input: {
          DEFAULT: 'rgb(var(--input))',
          border: 'rgb(var(--input-border))',
        },
        // Semantic colors
        success: 'rgb(var(--success))',
        warning: 'rgb(var(--warning))',
        error: 'rgb(var(--error))',
        info: 'rgb(var(--info))',
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'Consolas', 'monospace'],
      },
      fontSize: {
        '2xs': '0.625rem',
      },
      boxShadow: {
        'sm': 'var(--shadow-sm)',
        'DEFAULT': 'var(--shadow)',
        'md': 'var(--shadow-md)',
        'lg': 'var(--shadow-lg)',
        'xl': 'var(--shadow-xl)',
      },
      animation: {
        'fade-in': 'fadeIn 0.2s ease-out',
        'slide-in': 'slideIn 0.2s ease-out',
        'spin-slow': 'spin 2s linear infinite',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideIn: {
          '0%': { transform: 'translateY(-10px)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
    },
  },
  plugins: [],
};

