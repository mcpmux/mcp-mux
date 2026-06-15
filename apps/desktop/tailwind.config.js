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
        // McpMux brand colors — terra palette
        primary: {
          50: '#fff8f5',
          100: '#fef0e8',
          200: '#fde0d0',
          300: '#f5c7b0',
          400: '#E8956A',
          500: '#DA7756',
          600: '#C4704E',
          700: '#B8553A',
          800: '#954321',
          900: '#8B3D20',
          950: '#5C2810',
        },
        // McpMux extended brand palette
        mcpmux: {
          terracotta: '#DA7756',
          sienna: '#B8553A',
          'warm-coral': '#E8956A',
          cream: '#FFF5EE',
          mist: '#FDF2E9',
          wash: '#F5E0D0',
          dark: '#141416',
          'dark-surface': '#1E1410',
          'deep-cocoa': '#8B3D20',
          glow: '#F5C7B0',
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

