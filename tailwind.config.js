/** Tailwind statik derleme yapılandırması
 * src/index.html içindeki tüm sınıfları tarar.
 * WebView2 ortamında Tailwind Play CDN (dinamik <style>) çalışmadığı için
 * CSS çalışma zamanında değil derleme zamanında üretilir.
 * NOT: Renk paleti src/index.html içindeki tailwind.config ile AYNI tutulmalıdır.
 * Yeniden derle: npx tailwindcss@3.4 -c tailwind.config.js -i tw-input.css -o src/tailwind.css --minify
 */
/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/index.html"],
  theme: {
    extend: {
      colors: {
        slate:   { 200: '#c9d1d9', 300: '#e6edf3', 400: '#8b949e', 500: '#6e7681', 600: '#30363d', 700: '#21262d', 800: '#161b22', 900: '#0d1117', 950: '#010409' },
        cyan:    { 200: '#a5d6ff', 300: '#79c0ff', 400: '#58a6ff', 500: '#388bfd', 600: '#1f6feb' },
        emerald: { 300: '#7ee787', 400: '#3fb950', 500: '#2ea043', 600: '#238636' },
        red:     { 200: '#ffa198', 300: '#ff7b72', 400: '#f85149', 500: '#da3633', 600: '#b62324', 700: '#8b1a1a' },
        amber:   { 300: '#e3b341', 400: '#d29922', 500: '#bb8009', 600: '#9a6700' },
        violet:  { 300: '#d2b8ff', 400: '#bc8cff', 500: '#a371f7', 600: '#8957e5' },
        purple:  { 300: '#bc8cff', 400: '#a371f7', 500: '#8957e5', 600: '#6e40c9' },
        blue:    { 300: '#6cb6ff', 400: '#4098ff', 500: '#1f6feb', 600: '#1558b5' },
      },
    },
  },
  plugins: [],
};
