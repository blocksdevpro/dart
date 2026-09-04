import type { NextConfig } from "next";

const daemonUrl = (process.env.DART_DAEMON_URL || "http://127.0.0.1:4545").replace(/\/$/, "");

const nextConfig: NextConfig = {
  async rewrites() {
    return [
      {
        source: "/dart-api/:path*",
        destination: `${daemonUrl}/api/v1/:path*`,
      },
    ];
  },
};

export default nextConfig;
