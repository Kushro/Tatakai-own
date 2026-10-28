'use strict';
const path = require('node:path');
const verifyTranslationPackage = require('./verify-translate-package.cjs');

module.exports = async function verifyDesktopPackage(context) {
  await verifyTranslationPackage(context);
};
