import axios from "axios";
import OpenAI from "openai";
import "dotenv/config";


const OLLAMA_API = process.env.OLLAMA_API ?? "http://localhost:11434";
let USE_OLLAMA = false;

const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
});

const BB_API = process.env.BLUEBUBBLES_API ?? "http://localhost:12345";
const BB_PASSWORD = process.env.BLUEBUBBLES_PASSWORD;
const TRIGGER = (process.env.BOT_TRIGGER ?? "@ava").toLowerCase();

// Array of all triggers that should queue messages for processing
const TRIGGERS = [
  TRIGGER,
  "@character",
  "@unhinge"
];

// Message processing queue
const messageQueue = [];
let isProcessingQueue = false;

// Character contexts per chat
const chatCharacters = new Map();

async function generateCharacterPrompt(description) {
  try {
    const systemPrompt = `You are a prompt engineer. Generate a detailed system prompt for an AI character based on the user's description. The prompt should:
1. Define the character's personality, mannerisms, and speaking style
2. Include specific behavioral traits and quirks
3. Be detailed enough to create a consistent character persona
4. Start with "You are [character description]..."

Keep it concise but comprehensive. Return only the system prompt, nothing else.`;

    let completion;

    if (USE_OLLAMA) {
      const ollamaResponse = await axios.post(`${OLLAMA_API}/api/chat`, {
        model: process.env.OLLAMA_MODEL ?? "llama3.2",
        messages: [
          { role: "system", content: systemPrompt },
          { role: "user", content: description }
        ],
        stream: false
      });
      completion = ollamaResponse.data.message.content;
    } else {
      const openaiResponse = await openai.chat.completions.create({
        model: "gpt-4o",
        messages: [
          { role: "system", content: systemPrompt },
          { role: "user", content: description }
        ],
        temperature: 0.7
      });
      completion = openaiResponse.choices[0].message.content;
    }

    return completion.trim();
  } catch (error) {
    console.error("Error generating character prompt:", error);
    return `You are ${description}. Embody this character fully in your responses.`;
  }
}


async function parseCommand(text, chatGuid, context) {
  let command = false;
  console.log("HIT0")
  switch (true) {
    case /@character\s+(.+)/i.test(text):
      console.log("HIT")
      const characterMatch = text.match(/@character\s+(.+)/i);
      const description = characterMatch[1].trim();
      console.log(`Character switch requested: ${description}`);

      const characterPrompt = await generateCharacterPrompt(description);
      console.log(`Generated character prompt: ${characterPrompt.substring(0, 100)}...`);

      chatCharacters.set(chatGuid, characterPrompt);

      context.length = 0;

      await axios.post(`${BB_API}/api/v1/message/text?password=${BB_PASSWORD}`, {
        chatGuid: chatGuid,
        message: `✅ Character updated! I'm now: ${description} — MyAI`,
        tempGuid: `temp-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
      }, {
        headers: {
          'Content-Type': 'application/json'
        }
      });
      command = true;
      break;

    case /@unhinge\s+(.+)/i.test(text):
      const [, val] = text.match(/@unhinge\s+(.+)/i);
      const flag = val.trim().toLowerCase() === "true";
      USE_OLLAMA = flag;
      command = true;
      break;

    default:
      break;
  }
  return command;

}


async function queueMessageProcessing(chatGuid, text, chatContexts) {
  messageQueue.push({ chatGuid, text, timestamp: Date.now(), chatContexts });
  console.log(`Queued message processing: ${text.substring(0, 50)}... (Queue size: ${messageQueue.length})`);

  // Send immediate acknowledgment
  // await axios.post(`${BB_API}/api/v1/message/text?password=${BB_PASSWORD}`, {
  //   chatGuid: chatGuid,
  //   message: `Processing your message... (${messageQueue.length} in queue) — MyAI`,
  //   tempGuid: `temp-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
  // }, {
  //   headers: {
  //     'Content-Type': 'application/json'
  //   }
  // });

  processMessageQueue();
}

async function processMessageQueue() {
  if (isProcessingQueue || messageQueue.length === 0) return;

  isProcessingQueue = true;

  while (messageQueue.length > 0) {
    const { chatGuid, text, chatContexts } = messageQueue.shift();

    try {
      console.log(`Processing message: ${text.substring(0, 50)}...`);

      if (!chatContexts.has(chatGuid)) {
        chatContexts.set(chatGuid, []);
      }

      const context = chatContexts.get(chatGuid);


      const isCommand = await parseCommand(text, chatGuid, context);
      if (isCommand) {
        continue;
      }

      context.push({ role: "user", content: text });

      if (context.length > 10) {
        context.splice(0, context.length - 10);
      }

      let completion;

      const characterPrompt = chatCharacters.get(chatGuid) ||
        "You are MyAI, a casual assistant in a private friend group chat. Be brief and natural unless asked to elaborate. Match the group's tone and energy.";

      if (USE_OLLAMA) {
        console.log(`Using Ollama model: ${process.env.OLLAMA_MODEL ?? "llama2-uncensored"}`);
        console.log(`Ollama API: ${OLLAMA_API}`);

        const systemContent = characterPrompt + " If you want to generate and send a picture, just say [REQUEST_PICTURE] followed by your description.";

        const ollamaResponse = await axios.post(`${OLLAMA_API}/api/chat`, {
          model: process.env.OLLAMA_MODEL ?? "llama3.2",
          messages: [
            {
              role: "system",
              content: systemContent
            },
            ...context
          ],
          stream: false
        });

        console.log("Ollama response:", JSON.stringify(ollamaResponse.data, null, 2));

        completion = {
          choices: [{
            message: {
              content: ollamaResponse.data.message.content,
              tool_calls: null
            }
          }]
        };

        if (completion.choices[0].message.content.includes('[REQUEST_PICTURE]')) {
          const description = completion.choices[0].message.content.replace('[REQUEST_PICTURE]', '').trim();
          completion.choices[0].message.tool_calls = [{
            function: {
              name: "request_picture",
              arguments: JSON.stringify({ description })
            }
          }];
        }
      } else {
        const systemContent = characterPrompt + " If you want to generate and send a picture or image, use the request_picture tool with a detailed description of what image you want to create.";

        completion = await openai.chat.completions.create({
          model: "gpt-4o",
          messages: [
            {
              role: "system",
              content: systemContent
            },
            ...context
          ],
          tools: [
            {
              type: "function",
              function: {
                name: "request_picture",
                description: "Generate and send a picture to the chat using DALL-E",
                parameters: {
                  type: "object",
                  properties: {
                    description: {
                      type: "string",
                      description: "Detailed description of the picture to generate using DALL-E"
                    }
                  },
                  required: ["description"]
                }
              }
            }
          ]
        });
      }

      const response = completion.choices[0].message;
      let reply = response.content?.trim() || "";

      if (response.tool_calls) {
        for (const toolCall of response.tool_calls) {
          if (toolCall.function.name === "request_picture") {
            const args = JSON.parse(toolCall.function.arguments);
            const success = await generateAndSendPicture(chatGuid, args.description);
            if (success) {
              reply = `✅ Generated and sent a picture! — MyAI`;
            } else {
              reply = `❌ Failed to generate image. Please try again. — MyAI`;
            }
          }
        }
      }

      context.push({ role: "assistant", content: reply });

      await axios.post(`${BB_API}/api/v1/message/text?password=${BB_PASSWORD}`, {
        chatGuid: chatGuid,
        message: reply.includes("— MyAI") ? reply : `${reply} — MyAI`,
        tempGuid: `temp-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
      }, {
        headers: {
          'Content-Type': 'application/json'
        }
      });

    } catch (error) {
      console.error("Error processing queued message:", error);
      await axios.post(`${BB_API}/api/v1/message/text?password=${BB_PASSWORD}`, {
        chatGuid: chatGuid,
        message: `❌ Error processing message. Please try again. — MyAI`,
        tempGuid: `temp-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
      }, {
        headers: {
          'Content-Type': 'application/json'
        }
      });
    }

    await new Promise(resolve => setTimeout(resolve, 500));
  }

  isProcessingQueue = false;
}

async function generateAndSendPicture(chatGuid, description) {
  try {
    console.log(`Generating picture for description: ${description}`);

    // Generate image using OpenAI DALL-E
    const imageResponse = await openai.images.generate({
      model: "dall-e-3",
      prompt: description,
      n: 1,
      size: "1024x1024",
      quality: "standard"
    });

    const imageUrl = imageResponse.data[0].url;

    // Download the generated image
    const imageData = await axios.get(imageUrl, { responseType: 'arraybuffer' });
    const imageBuffer = Buffer.from(imageData.data);

    // Send the image via BlueBubbles API
    const formData = new FormData();
    const fileName = 'generated-image.png';
    const tempGuid = `temp-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

    // Create a blob from the buffer
    const imageBlob = new Blob([imageBuffer], { type: 'image/png' });

    formData.append('chatGuid', chatGuid);
    formData.append('tempGuid', tempGuid);
    formData.append('name', fileName);
    formData.append('attachment', imageBlob, fileName);

    await axios.post(`${BB_API}/api/v1/message/attachment?password=${BB_PASSWORD}`, formData);

    return true;
  } catch (error) {
    console.error("Error generating/sending picture:", error);
    return false;
  }
}

async function pollMessages() {
  const seen = new Set();
  const chatContexts = new Map();
  const processedMessages = new Set();
  const startupTime = Date.now();

  setInterval(async () => {
    try {
      // Get recent chats with their messages
      const { data } = await axios.post(`${BB_API}/api/v1/chat/query?password=${BB_PASSWORD}`, {
        limit: 500,
        offset: 0,
        with: ["lastMessage"],
        sort: "lastmessage"
      }, {
        headers: {
          'Content-Type': 'application/json'
        }
      });
      const chats = data?.data || [];

      for (const chat of chats) {
        // Get recent messages for this chat to handle multiple triggers
        const messagesData = await axios.post(`${BB_API}/api/v1/message/query?password=${BB_PASSWORD}`, {
          chatGuid: chat.guid,
          limit: 50,
          offset: 0,
          sort: "DESC"
        }, {
          headers: {
            'Content-Type': 'application/json'
          }
        });

        const messages = messagesData.data?.data || [];

        for (const msg of messages.reverse()) {
          if (!msg.guid || seen.has(msg.guid)) continue;
          seen.add(msg.guid);

          // Skip messages that are older than bot startup time
          const messageTime = msg.dateCreated || msg.dateDelivered || 0;
          if (messageTime < startupTime) continue;

          const text = msg.text ?? "";
          const lowerText = text.toLowerCase();
          
          // Check if message contains any of the triggers
          const containsTrigger = TRIGGERS.some(trigger => lowerText.includes(trigger.toLowerCase()));
          
          if (containsTrigger && !processedMessages.has(msg.guid)) {
            processedMessages.add(msg.guid);
            console.log("[MyAI trigger]", text);

            // Queue the message for processing
            await queueMessageProcessing(chat.guid, text, chatContexts);
          }
        }
      }

      // Clean up seen messages
      if (seen.size > 1_000) {
        const last500 = Array.from(seen).slice(-500);
        seen.clear();
        last500.forEach((g) => seen.add(g));
      }

      // Clean up processed messages
      if (processedMessages.size > 1_000) {
        const last500 = Array.from(processedMessages).slice(-500);
        processedMessages.clear();
        last500.forEach((g) => processedMessages.add(g));
      }

      // Clean up chat contexts (keep only last 20 chats)
      if (chatContexts.size > 20) {
        const chatKeys = Array.from(chatContexts.keys());
        const oldestChats = chatKeys.slice(0, chatKeys.length - 20);
        oldestChats.forEach(chatGuid => chatContexts.delete(chatGuid));
      }
    } catch (err) {
      console.log(err);
      console.error("Poll error:", err.response?.data || err.message);
    }
  }, 3_000);
}

pollMessages();
