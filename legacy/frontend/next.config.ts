import type { NextConfig } from "next";

const coreUrl = process.env.CORE_URL || "http://127.0.0.1:4000";

const nextConfig: NextConfig = {
  output: "standalone",
  turbopack: {
    root: __dirname,
  },
  async rewrites() {
    return [
      {
        source: "/api/core/:path*",
        destination: `${coreUrl}/api/core/:path*`,
      },
      {
        source: "/api/app/:path*",
        destination: `${coreUrl}/api/app/:path*`,
      },
      {
        source: "/internal/:path*",
        destination: `${coreUrl}/internal/:path*`,
      },
    ];
  },
};

export default nextConfig;
