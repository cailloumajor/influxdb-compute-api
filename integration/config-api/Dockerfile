FROM node:22.14-alpine

WORKDIR /usr/src

COPY package.json ./

# hadolint ignore=DL3060
RUN yarn install --no-lockfile

EXPOSE 3000
CMD ["yarn", "start"]
