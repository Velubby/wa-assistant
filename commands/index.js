// Static command registry - generated at build time, no dynamic require()
const fs = require('fs');
const path = require('path');

// Import all command modules statically
const draw = require('./draw.js');
const facebook = require('./facebook.js');
const fake = require('./fake.js');
const getpdf = require('./getpdf.js');
const getsticker = require('./getsticker.js');
const gift = require('./gift.js');
const groupname = require('./groupname.js');
const help = require('./help.js');
const ig = require('./ig.js');
const jarvis = require('./jarvis.js');
const kick = require('./kick.js');
const ocr = require('./ocr.js');
const ping = require('./ping.js');
const qr = require('./qr.js');
const remind = require('./remind.js');
const rvo = require('./rvo.js');
const stalkgh = require('./stalkgh.js');
const stalkig = require('./stalkig.js');
const stalktt = require('./stalktt.js');
const sticker = require('./sticker.js');
const tagall = require('./tagall.js');
const tempmail = require('./tempmail.js');
const tiktok = require('./tiktok.js');
const to = require('./to.js');
const toanime = require('./toanime.js');
const tod = require('./tod.js');
const toimg = require('./toimg.js');
const toprofile = require('./toprofile.js');
const tosticker = require('./tosticker.js');
const twitter = require('./twitter.js');
const yt = require('./yt.js');

const allCommands = [
  draw, facebook, fake, getpdf, getsticker, gift, groupname, help, ig, jarvis,
  kick, ocr, ping, qr, remind, rvo, stalkgh, stalkig, stalktt, sticker,
  tagall, tempmail, tiktok, to, toanime, tod, toimg, toprofile, tosticker,
  twitter, yt
].filter(cmd => !cmd.hide);

const commandMap = new Map(allCommands.map(cmd => [cmd.name, cmd]));

module.exports = {
  allCommands,
  commandMap,
  getCommand: (name) => commandMap.get(name),
  getVisibleCommands: () => allCommands
};