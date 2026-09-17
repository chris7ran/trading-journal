/**
 * Raise the iOS deployment target on **every** Pods target, resource bundles
 * included.
 *
 * # Why this exists
 *
 * `expo-build-properties` sets `ios.deploymentTarget` on the app target and on
 * the pod targets, but not on the *resource bundle* targets CocoaPods generates
 * alongside them — `RNSVG-RNSVGFilters` and friends. Those keep whatever the
 * podspec declared, often iOS 12.4, and recent Xcode versions refuse to build
 * anything below their own floor. The failure names a target you never wrote
 * and cannot configure from `app.json`.
 *
 * Patching it by hand in Xcode works until the next `expo prebuild`, which
 * regenerates `ios/` and throws the fix away. Doing it here means the Podfile
 * carries the correction, so it survives every regeneration.
 *
 * # How
 *
 * A dangerous mod appends a loop to the Podfile's existing `post_install`
 * block. The loop only ever raises a target's floor, never lowers it, so a pod
 * that legitimately requires a newer iOS keeps its own value.
 *
 * Usage in `app.json`:
 *
 *   ["./plugins/withMinimumIosDeploymentTarget", { "deploymentTarget": "16.0" }]
 */

const fs = require('fs');
const path = require('path');
const { withDangerousMod } = require('expo/config-plugins');

const DEFAULT_TARGET = '16.0';
const MARKER = '# expo-config-plugin: minimum iOS deployment target';

module.exports = function withMinimumIosDeploymentTarget(config, props = {}) {
  const deploymentTarget = props.deploymentTarget || DEFAULT_TARGET;

  return withDangerousMod(config, [
    'ios',
    (cfg) => {
      const podfilePath = path.join(cfg.modRequest.platformProjectRoot, 'Podfile');
      let contents = fs.readFileSync(podfilePath, 'utf8');

      // Prebuild can run more than once against the same Podfile.
      if (contents.includes(MARKER)) {
        return cfg;
      }

      const anchor = 'post_install do |installer|';
      if (!contents.includes(anchor)) {
        throw new Error(
          `withMinimumIosDeploymentTarget: no "${anchor}" block found in ${podfilePath}. ` +
            'The Expo Podfile template has changed and this plugin needs updating.',
        );
      }

      const patch = `${anchor}
    ${MARKER}
    # Resource bundle targets are not covered by expo-build-properties, and a
    # single one left at an old floor fails the whole build.
    installer.pods_project.targets.each do |target|
      target.build_configurations.each do |config|
        current = config.build_settings['IPHONEOS_DEPLOYMENT_TARGET'].to_f
        if current < ${parseFloat(deploymentTarget)}
          config.build_settings['IPHONEOS_DEPLOYMENT_TARGET'] = '${deploymentTarget}'
        end
      end
    end
`;

      contents = contents.replace(anchor, patch);
      fs.writeFileSync(podfilePath, contents);
      return cfg;
    },
  ]);
};
