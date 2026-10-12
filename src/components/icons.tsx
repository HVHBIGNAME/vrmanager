/**
 * Icon set.
 *
 * Inline SVG, single stroke weight (1.6), `currentColor` only — so icons
 * inherit text colour and stay consistent across the app. No icon library
 * dependency for a dozen glyphs.
 */
import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 16, children, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.6}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  );
}

export const IconCompass = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="9" />
    <path d="m15.5 8.5-2.1 4.9-4.9 2.1 2.1-4.9z" />
  </Svg>
);

export const IconLibrary = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="4" width="7" height="16" rx="1.5" />
    <rect x="14" y="4" width="7" height="10" rx="1.5" />
    <path d="M14 18h7" />
  </Svg>
);

export const IconHeadset = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 11.5A3.5 3.5 0 0 1 6.5 8h11A3.5 3.5 0 0 1 21 11.5v2.2a2.3 2.3 0 0 1-2.3 2.3h-1.4a2 2 0 0 1-1.6-.8L14.4 14h-4.8l-1.3 1.2a2 2 0 0 1-1.6.8H5.3A2.3 2.3 0 0 1 3 13.7z" />
    <path d="M7.5 8V7a4.5 4.5 0 0 1 9 0v1" />
  </Svg>
);

export const IconDownload = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3v12" />
    <path d="m7.5 10.5 4.5 4.5 4.5-4.5" />
    <path d="M4 20h16" />
  </Svg>
);

export const IconStar = (p: IconProps) => (
  <Svg {...p}>
    <path d="m12 3.5 2.6 5.5 6 .8-4.3 4.2 1 6-5.3-2.9-5.3 2.9 1-6L3.4 9.8l6-.8z" />
  </Svg>
);

export const IconSettings = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2.5v2.2M12 19.3v2.2M4.2 7l1.9 1.1M17.9 15.9l1.9 1.1M4.2 17l1.9-1.1M17.9 8.1 19.8 7" />
  </Svg>
);

export const IconSearch = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m16 16 4.5 4.5" />
  </Svg>
);

export const IconChevron = (p: IconProps) => (
  <Svg {...p}>
    <path d="m6 9.5 6 5 6-5" />
  </Svg>
);

export const IconUsb = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="19" r="1.6" />
    <path d="M12 17.4V6" />
    <path d="m9 9 3-5 3 5" />
    <path d="M8 13.5h3.9M16 11.5h-4" />
  </Svg>
);

export const IconWifi = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 9.5a12 12 0 0 1 16 0" />
    <path d="M7 13a8 8 0 0 1 10 0" />
    <path d="M10 16.3a3.6 3.6 0 0 1 4 0" />
    <circle cx="12" cy="19.4" r="0.8" fill="currentColor" stroke="none" />
  </Svg>
);

export const IconPlay = (p: IconProps) => (
  <Svg {...p}>
    <path d="M8 5.5v13l11-6.5z" />
  </Svg>
);

export const IconTrash = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 7h16" />
    <path d="M9 7V4.8h6V7" />
    <path d="M6 7l1 13h10l1-13" />
  </Svg>
);

export const IconStop = (p: IconProps) => (
  <Svg {...p}>
    <rect x="6.5" y="6.5" width="11" height="11" rx="2" />
  </Svg>
);

export const IconRefresh = (p: IconProps) => (
  <Svg {...p}>
    <path d="M20 12a8 8 0 1 1-2.6-5.9" />
    <path d="M20 4v4.5h-4.5" />
  </Svg>
);

export const IconPlus = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 5v14M5 12h14" />
  </Svg>
);

export const IconCheck = (p: IconProps) => (
  <Svg {...p}>
    <path d="m5 12.8 4.4 4.2L19 7.5" />
  </Svg>
);

export const IconX = (p: IconProps) => (
  <Svg {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Svg>
);

export const IconPackage = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12 3l8 4.3v9.4L12 21l-8-4.3V7.3z" />
    <path d="M4.3 7.4 12 11.6l7.7-4.2M12 11.6V21" />
  </Svg>
);

export const IconBattery = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="8" width="16" height="8" rx="2" />
    <path d="M21 11v2" />
  </Svg>
);

export const IconStorage = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3" y="5" width="18" height="6" rx="1.8" />
    <rect x="3" y="13" width="18" height="6" rx="1.8" />
    <path d="M7 8h.01M7 16h.01" />
  </Svg>
);
